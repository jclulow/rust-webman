use super::common::*;
use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Mandir {
    cat: Catalogue,
    manpath: Vec<PathBuf>,
    mandoc: PathBuf,
    sections: BTreeSet<String>,
    subsections: BTreeSet<String>,
}

#[derive(Debug, Deserialize)]
struct Catalogue {
    sections: BTreeMap<String, String>,
    subsections: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
pub struct TocSection {
    pub name: String,
    pub title: Option<String>,
    pub subsections: Vec<TocSubsection>,
}

#[derive(Debug, Serialize)]
pub struct TocSubsection {
    pub name: String,
    pub title: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SubsectionLookup {
    pub name: String,
    pub title: Option<String>,
    pub redirect: bool,
}

#[derive(Debug)]
pub struct PageLookup {
    pub section: String,
    pub name: String,
    pub path: PathBuf,
    pub redirect: bool,
}

impl Mandir {
    pub fn new<P1, P2>(cat: P1, mandoc: P2) -> Result<Mandir>
    where
        P1: AsRef<Path>,
        P2: AsRef<Path>,
    {
        let catpath = cat.as_ref();
        let cat: Catalogue = jmclib::toml::read_file(catpath)?
            .ok_or(anyhow!("catalogue file {}", catpath.display()))?;

        for (s, _) in cat.sections.iter() {
            if &s.to_uppercase() != s {
                bail!("section {s:?} should be all uppercase");
            }
        }

        for (ss, _) in cat.subsections.iter() {
            if &ss.to_uppercase() != ss {
                bail!("subsection {ss:?} should be all uppercase");
            }
        }

        Ok(Mandir {
            cat,
            manpath: Vec::new(),
            mandoc: mandoc.as_ref().to_path_buf(),
            sections: BTreeSet::new(),
            subsections: BTreeSet::new(),
        })
    }

    pub fn add_mandir<P: AsRef<Path>>(&mut self, manpath: P) -> Result<()> {
        let manpath = manpath.as_ref();

        let mut rd = std::fs::read_dir(manpath)?;
        while let Some(ent) = rd.next().transpose()? {
            if !ent.file_type()?.is_dir() {
                continue;
            }

            let n = ent.file_name().to_str().unwrap().to_string();

            if !n.starts_with("man") {
                continue;
            }

            /*
             * Every top-level section is also considered here as a subsection,
             * as some sections contain pages at the top level (e.g., section 9)
             * as well as in subsections (e.g., subsection 9F).
             */
            self.sections.insert(n[3..4].to_string().to_uppercase());
            self.subsections.insert(n[3..].to_string().to_uppercase());
        }

        self.manpath.push(manpath.to_path_buf());

        Ok(())
    }

    pub fn pages(&self, sect: &str) -> Result<Vec<String>> {
        let sect = sect.trim().to_lowercase();
        let trailer = format!(".{}", sect);
        let mut pagelist: Vec<String> = Vec::new();

        for mandir in self.manpath.iter() {
            let mut d = mandir.clone();
            d.push(&format!("man{}", sect));

            let mut rd = std::fs::read_dir(&d)?;
            while let Some(ent) = rd.next().transpose()? {
                let n = ent
                    .file_name()
                    .to_str()
                    .unwrap()
                    .trim_end_matches(&trailer)
                    .to_string();
                pagelist.push(n);
            }
        }

        pagelist.sort();

        Ok(pagelist)
    }

    pub fn index(&self) -> Result<Vec<TocSection>> {
        let mut out = Vec::new();

        for sect in self.sections.iter() {
            let name = sect.to_string();
            let title = self.cat.sections.get(&name).map(|s| s.to_string());

            let subsections = self
                .subsections
                .iter()
                .filter(|ss| ss.starts_with(sect))
                .map(|ss| TocSubsection {
                    name: ss.to_string(),
                    title: self.cat.subsections.get(ss).map(|s| s.to_string()),
                })
                .collect();

            out.push(TocSection { name, title, subsections });
        }

        Ok(out)
    }

    fn lookup_subsection_impl(
        &self,
        uname: &str,
        redirect: bool,
    ) -> Option<SubsectionLookup> {
        if self.subsections.contains(uname) {
            let mut title = Vec::new();

            if let Some(s) =
                self.sections.iter().find(|s| uname.starts_with(*s))
            {
                if let Some(t) = self.cat.sections.get(s) {
                    title.push(t.to_string());
                }
            }

            if let Some(t) = self.cat.subsections.get(uname) {
                if !title.contains(&t) {
                    title.push(t.to_string());
                }
            }

            let title =
                if title.is_empty() { None } else { Some(title.join(": ")) };

            Some(SubsectionLookup { name: uname.to_string(), title, redirect })
        } else {
            None
        }
    }

    pub fn lookup_subsection(
        &self,
        name: &str,
    ) -> Result<Option<SubsectionLookup>> {
        if name.is_empty() {
            return Ok(None);
        }

        /*
         * Section names are canonically rendered in uppercase.
         */
        let uname = name.trim().to_uppercase();
        let redirect = name != uname;

        if let Some(res) = self.lookup_subsection_impl(&uname, redirect) {
            return Ok(Some(res));
        }

        /*
         * Some entire subsections were renamed previously.  If there was no
         * direct match, check for a potential renamed match:
         */
        let prefix = uname.chars().next().unwrap();
        let tail = uname.chars().skip(1).collect::<String>();

        let redir = match prefix {
            '1' if tail == "M" => Some("8".to_string()),
            '4' => Some(format!("5{tail}")),
            '5' => Some(format!("7{tail}")),
            '7' => Some(format!("4{tail}")),
            _ => None,
        };

        if let Some(redir) = redir {
            if let Some(res) = self.lookup_subsection_impl(&redir, true) {
                return Ok(Some(res));
            }
        }

        Ok(None)
    }

    fn lookup_file(
        &self,
        sects: &[String],
        page: &str,
        redirect: bool,
    ) -> Result<Vec<PageLookup>> {
        let mut out = Vec::new();

        for mandir in self.manpath.iter() {
            for sect in sects.iter() {
                let lsect = sect.to_lowercase();
                let fp = mandir
                    .join(&format!("man{lsect}"))
                    .join(&format!("{page}.{lsect}"));

                match std::fs::metadata(&fp) {
                    Ok(st) if st.is_file() => {
                        out.push(PageLookup {
                            section: sect.to_string(),
                            name: page.to_string(),
                            path: fp,
                            redirect,
                        });
                    }
                    _ => continue,
                }
            }
        }

        Ok(out)
    }

    pub fn lookup_page(
        &self,
        sect: Option<&SubsectionLookup>,
        page: &str,
    ) -> Result<Vec<PageLookup>> {
        if page.contains('/') {
            bail!("invalid page: {page}");
        }

        let mut redirect = false;
        let mut sects = Vec::with_capacity(self.subsections.len());
        if let Some(sect) = sect {
            /*
             * If we had to adjust the subsection case, we will want to redirect
             * the user to the canonical URL.
             */
            redirect = sect.redirect;

            sects.push(sect.name.clone());
        } else {
            /*
             * If no subsection was specified, use the default search order.
             */
            self.subsections
                .iter()
                .for_each(|sect| sects.push(sect.to_string()));
        }

        let pages = self.lookup_file(&sects, page, redirect)?;
        if !pages.is_empty() {
            return Ok(pages);
        }

        /*
         * If we could not locate the requested page in the chosen search path,
         * check to see if the page may have existed in a section that has since
         * been renamed:
         */
        if sect.is_some() && sects.len() == 1 {
            let prefix = sects[0].chars().next().unwrap();
            let tail = sects[0].chars().skip(1).collect::<String>();

            let redir = match prefix {
                '1' => {
                    if tail == "m" || tail == "M" {
                        Some("8".to_string())
                    } else {
                        None
                    }
                }
                '4' => Some(format!("5{tail}")),
                '5' => Some(format!("7{tail}")),
                '7' => Some(format!("4{tail}")),
                _ => None,
            };

            if let Some(redir) = redir {
                let res = self.lookup_file(&[redir], page, true)?;
                if !res.is_empty() {
                    return Ok(res);
                }
            }
        }

        Ok(Default::default())
    }
}
