use super::common::*;
use std::path::{Path, PathBuf};
use anyhow::{Result, anyhow, bail};
use std::collections::{BTreeMap, BTreeSet};
use serde::Deserialize;

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

#[derive(Debug)]
pub struct TocSection {
    pub name: String,
    pub title: Option<String>,
    pub subsections: Vec<TocSubsection>,
}

#[derive(Debug)]
pub struct TocSubsection {
    pub name: String,
    pub title: Option<String>,
}

impl Mandir {
    pub fn new<P1, P2>(cat: P1, mandoc: P2)
        -> Result<Mandir>
        where
            P1: AsRef<Path>,
            P2: AsRef<Path>,
    {
        let catpath = cat.as_ref();
        let cat: Catalogue = jmclib::toml::read_file(catpath)?
            .ok_or(anyhow!("catalogue file {}", catpath.display()))?;

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

            let n = ent
                .file_name().to_str().unwrap()
                .to_string();

            if !n.starts_with("man") {
                continue;
            }

            self.sections.insert(n[3..4].to_string());
            self.subsections.insert(n[3..].to_string());
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
                    .file_name().to_str().unwrap()
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

            let subsections = self.subsections.iter()
                .filter(|ss| ss.starts_with(sect))
                .map(|ss| TocSubsection {
                    name: ss.to_string(),
                    title: self.cat.subsections.get(ss)
                        .map(|s| s.to_string()),
                })
                .collect();

            out.push(TocSection { name, title, subsections });
        }

        Ok(out)
    }

    pub fn lookup(&self, sect: Option<&str>, page: &str) -> Result<PathBuf> {
        /*
         * First, validate the section if one was provided.
         */
        let sects = if let Some(sect) = &sect {
            if !self.subsections.contains(*sect) {
                bail!("unknown section: {}", sect);
            }
            vec![sect.to_string()]
        } else {
            self.subsections.iter().map(|s| s.to_string()).collect()
        };

        if page.contains('/') {
            bail!("invalid page: {}", page);
        }

        for mandir in self.manpath.iter() {
            for sect in sects.iter() {
                let mut fp = mandir.clone();
                fp.push(&format!("man{}", sect));
                fp.push(&format!("{}.{}", page, sect));

                match std::fs::metadata(&fp) {
                    Ok(st) if st.is_file() => return Ok(fp),
                    _ => continue,
                }
            }
        }

        bail!("page not found");
    }
}
