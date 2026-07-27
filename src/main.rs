/*
 * Copyright 2026 Oxide Computer Company
 */

use std::path::Path;
use std::sync::Arc;
use std::sync::LazyLock;

use anyhow::{Result, anyhow, bail};
use dropshot::{
    ApiDescription, Body, ConfigDropshot, HttpError, HttpServerStarter,
    Path as TypedPath, RequestContext, endpoint,
};
use http::Response;
use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use slog::info;
use tera::Context;
use tokio::process::Command;

mod common;
use common::*;
mod mandir;
mod typewriter;

const MANDOC: &str = "/usr/bin/mandoc";

type DSResult<T> = std::result::Result<T, dropshot::HttpError>;

fn redirect(path: &str) -> DSResult<Response<Body>> {
    Ok(hyper::Response::builder()
        .status(hyper::StatusCode::SEE_OTHER)
        .header(hyper::header::LOCATION, path)
        .body(dropshot::Body::empty())?)
}

struct App {
    mandir: mandir::Mandir,
    templates: tera::Tera,
}

#[tokio::main]
async fn main() -> Result<()> {
    let opts = getopts::Options::new()
        .optopt("b", "", "bind address:port", "BIND_ADDRESS")
        .optmulti("M", "", "add manual page directory", "MANPATH")
        .parsing_style(getopts::ParsingStyle::StopAtFirstFree)
        .parse(std::env::args_os().skip(1))?;

    if !opts.free.is_empty() {
        bail!("unexpected arguments");
    }

    let mut ad = ApiDescription::new();
    ad.register(page_index_get)?;
    ad.register(page_section_get)?;
    ad.register(page_page_get)?;

    let bind_address =
        opts.opt_str("b").as_deref().unwrap_or("0.0.0.0:5583").parse()?;

    let log = make_log("webman", "WEBMAN_DEBUG");

    let mut templates = tera::Tera::new();
    templates.load_from_glob(
        jmclib::dirs::rootpath("share/web/**/*.*")?.to_str().unwrap(),
    )?;

    let cat = jmclib::dirs::rootpath("share/manual.toml")?;
    let mut md = mandir::Mandir::new(&cat)?;

    let mandirs = opts.opt_strs("M");
    if mandirs.is_empty() {
        md.add_mandir("/usr/share/man")?;
    } else {
        for m in mandirs {
            md.add_mandir(&m)?;
        }
    }

    info!(log, "mandir: {md:#?}");
    info!(log, "index: {:#?}", md.index()?);

    let a = Arc::new(App { mandir: md, templates });

    let s = HttpServerStarter::new(
        &ConfigDropshot {
            bind_address,
            log_headers: vec![
                "X-Forwarded-For".into(),
                "X-Real-Ip".into(),
                "Referer".into(),
            ],
            ..Default::default()
        },
        ad,
        a,
        &log,
    )
    .map_err(|e| anyhow!("server startup failure: {e}"))?;

    match s.start().await {
        Ok(()) => bail!("server stopped early"),
        Err(e) => bail!("server error? {e}"),
    }
}

impl App {
    fn render_template(
        &self,
        name: &str,
        mut ctx: Context,
    ) -> DSResult<Response<Body>> {
        ctx.insert("template", name);

        let out = self
            .templates
            .render(name, &ctx)
            .map_err(|e| HttpError::for_internal_error(e.to_string()))?
            .trim_start()
            .to_string();

        Ok(hyper::Response::builder()
            .status(hyper::StatusCode::OK)
            .header(hyper::header::CONTENT_TYPE, "text/html; charset=utf-8")
            .header(hyper::header::CONTENT_LENGTH, out.len())
            .body(dropshot::Body::from(out))?)
    }
}

#[endpoint {
    method = GET,
    path = "/man",
}]
pub async fn page_index_get(
    rc: RequestContext<Arc<App>>,
) -> DSResult<Response<Body>> {
    let a = rc.context();

    let mut ctx = Context::default();
    ctx.insert(
        "catalogue",
        &a.mandir.index().map_err(|e| {
            HttpError::for_internal_error(format!("catalogue: {e}"))
        })?,
    );

    a.render_template("index.html", ctx)
}

#[derive(Deserialize, JsonSchema)]
pub struct SectionPath {
    section: String,
}

#[endpoint {
    method = GET,
    path = "/man/{section}",
}]
pub async fn page_section_get(
    rc: RequestContext<Arc<App>>,
    path: TypedPath<SectionPath>,
) -> DSResult<Response<Body>> {
    let a = rc.context();

    let section = path.into_inner().section.trim().to_string();
    let subsect = match section.as_str() {
        "" | "all" => return redirect("/man"),
        other => {
            if let Some(sect) =
                a.mandir.lookup_subsection(other).map_err(|e| {
                    HttpError::for_internal_error(format!(
                        "subsection lookup: {e}"
                    ))
                })?
            {
                /*
                 * This is the name of a section or a subsection, so display the
                 * index.
                 */
                sect
            } else {
                /*
                 * Otherwise, try to resolve the section name as a page name
                 * using the default section search order.
                 */
                let pages =
                    a.mandir.lookup_page(None, &other).map_err(|e| {
                        HttpError::for_internal_error(format!(
                            "page lookup: {e}"
                        ))
                    })?;
                if pages.len() == 1 {
                    /*
                     * Redirect the user to the canonical URL for the page they
                     * have requested:
                     */
                    return redirect(&format!(
                        "/man/{}/{}",
                        pages[0].section, pages[0].name
                    ));
                } else if pages.len() > 1 {
                    /*
                     * More than one section contains a matching page!
                     */
                    let mut ctx = Context::default();

                    ctx.insert("name", &other);
                    ctx.insert(
                        "sections",
                        &pages
                            .into_iter()
                            .filter_map(|p| {
                                a.mandir
                                    .lookup_subsection(&p.section)
                                    .ok()
                                    .flatten()
                                    .map(|s| {
                                        let heading = if let Some(t) = s.title {
                                            format!("section {}: {t}", s.name)
                                        } else {
                                            format!("section {}", s.name)
                                        };

                                        WhichSection {
                                            name: s.name,
                                            heading,
                                            pages: vec![p.name],
                                        }
                                    })
                            })
                            .collect::<Vec<_>>(),
                    );

                    return a.render_template("which.html", ctx);
                }

                return Err(HttpError::for_not_found(
                    None,
                    format!("section {other:?} not found"),
                ));
            }
        }
    };

    if subsect.redirect {
        /*
         * Redirect the user to the canonical URL for the subsection they have
         * requested:
         */
        return redirect(&format!("/man/{}", subsect.name));
    }

    let pages = a.mandir.pages(&subsect.name).map_err(|e| {
        HttpError::for_internal_error(format!("pages lookup: {e}"))
    })?;

    let mut ctx = Context::default();
    ctx.insert("name", &subsect.name);
    ctx.insert("title", &subsect.title);
    ctx.insert("pages", &pages);

    a.render_template("section.html", ctx)
}

#[derive(Deserialize, JsonSchema)]
pub struct SectionPagePath {
    section: String,
    page: String,
}

fn anchor_name(h: &str) -> String {
    h.to_lowercase().replace(' ', "-").replace('_', "-")
}

async fn render(path: &Path, width: u32) -> DSResult<String> {
    let res = Command::new(MANDOC)
        .env_clear()
        .env("LANG", "C.UTF-8")
        .env("LC_ALL", "C.UTF-8")
        .arg("-Tascii")
        .arg(&format!("-Owidth={width}"))
        .arg(path)
        .output()
        .await
        .map_err(|e| HttpError::for_internal_error(format!("mandoc: {e}")))?;

    if !res.status.success() {
        return Err(HttpError::for_internal_error(format!(
            "mandoc: {}",
            res.info()
        )));
    }

    let s = String::from_utf8(res.stdout).map_err(|e| {
        HttpError::for_internal_error(format!("mandoc output: {e}"))
    })?;
    let mut t = typewriter::Typewriter::new();
    for c in s.chars() {
        t.append(c);
    }

    static RE_H2: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^<b>([-A-Z0-9_ ]{2,})</b>$").unwrap());
    static RE_H3: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^( {3})<b>([-A-Za-z0-9_ ]{2,})</b>$").unwrap()
    });
    static RE_H4: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^( {5})<b>Example ([0-9]+) *</b> *(.*)$").unwrap()
    });

    static RE_CR1: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"<b>([.a-zA-Z0-9_-]{2,})</b>\(([0-9A-Z]+)\)").unwrap()
    });
    static RE_CR2: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"<b>([.a-zA-Z0-9_-]{2,})\(([0-9A-Z]+)\)</b>").unwrap()
    });
    static RE_CR3: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"([a-zA-Z][a-zA-Z0-9_.-]+)\(([0-9A-Z]+)\)").unwrap()
    });

    let mut html = String::new();
    for (i, l) in t.to_html(false).into_iter().enumerate() {
        html.push_str(&if let Some(g) = RE_H2.captures(&l) {
            format!(
                "<a name=\"{an}\"></a>\
                <a class=\"anc\" href=\"#{an}\"><h2>{h}</h2></a>",
                an = anchor_name(&g[1]),
                h = &g[1],
            )
        } else if let Some(g) = RE_H3.captures(&l) {
            format!(
                "<a name=\"{an}\"></a>\
                <a class=\"anc\" href=\"#{an}\"><h3>{i}{h}</h3></a>",
                i = &g[1],
                an = anchor_name(&g[2]),
                h = &g[2],
            )
        } else if let Some(g) = RE_H4.captures(&l) {
            let an = anchor_name(&format!("example-{}", &g[1]));
            format!(
                "<a name=\"{an}\"></a>\
                <a class=\"anc\" href=\"#{an}\">\
                <h4>{i}<b>Example {n}</b> {x}</h4>\
                </a>",
                i = &g[1],
                n = &g[2],
                x = &g[3],
            )
        } else if i > 0 {
            let link = "<a href=\"/man/$2/$1\">$1($2)</a>";
            let mut l = RE_CR1.replace_all(&l, link).to_string();
            l = RE_CR2.replace_all(&l, link).to_string();
            l = RE_CR3.replace_all(&l, link).to_string();
            l.to_string()
        } else {
            l
        });

        html.push('\n');
    }

    Ok(html)
}

#[derive(Serialize)]
struct WhichSection {
    name: String,
    heading: String,
    pages: Vec<String>,
}

#[endpoint {
    method = GET,
    path = "/man/{section}/{page}",
}]
pub async fn page_page_get(
    rc: RequestContext<Arc<App>>,
    path: TypedPath<SectionPagePath>,
) -> DSResult<Response<Body>> {
    let a = rc.context();
    let path = path.into_inner();

    let Some(subsect) =
        a.mandir.lookup_subsection(&path.section).map_err(|e| {
            HttpError::for_internal_error(format!("subsection lookup: {e}"))
        })?
    else {
        return Err(HttpError::for_not_found(
            None,
            format!("section {:?} not found", path.section),
        ));
    };

    let pages = match path.page.as_str() {
        "" | "all" => {
            return Ok(hyper::Response::builder()
                .status(hyper::StatusCode::SEE_OTHER)
                .header(
                    hyper::header::LOCATION,
                    &format!("/man/{}", subsect.name.to_uppercase()),
                )
                .body(dropshot::Body::empty())?);
        }
        other => a.mandir.lookup_page(Some(&subsect), other).map_err(|e| {
            HttpError::for_internal_error(format!("page lookup: {e}"))
        })?,
    };

    if pages.is_empty() {
        return Err(HttpError::for_not_found(
            None,
            format!("page {:?} not found", path.page),
        ));
    };
    let page = &pages[0];

    if page.redirect {
        return Ok(hyper::Response::builder()
            .status(hyper::StatusCode::SEE_OTHER)
            .header(
                hyper::header::LOCATION,
                &format!("/man/{}/{}", page.section.to_uppercase(), page.name),
            )
            .body(dropshot::Body::empty())?);
    }

    let cols80 = render(&page.path, 80).await?;
    let cols60 = render(&page.path, 60).await?;

    let mut ctx = Context::default();
    ctx.insert("name", &page.name);
    ctx.insert("section", &page.section);
    ctx.insert("content80", &cols80);
    ctx.insert("content60", &cols60);

    a.render_template("page.html", ctx)
}
