// SPDX-License-Identifier: GPL-2.0-or-later
//! The web pages Washi can point a person to: the ink shop, the manual, help for a support code,
//! and the printer's own web page with its maintenance menu.
//!
//! The first three come from the printer itself (`/JS_MDL/model.js`, see [`washi_net::web`]); the
//! addresses are specific to the model, so Washi does not guess them. When the printer does not
//! offer one (or cannot be read), the link falls back to the printer's own web page
//! `http://<host>/`, which has its own links. Opening any of these is up to the person: the app
//! asks first and says whether the page is the maker's or the printer's.

use std::collections::BTreeMap;

use serde::Serialize;
use washi_net::web::{ModelLinks, origin, page_url};

use crate::lang::Language;
use crate::printer::Printer;

/// Whose page a link leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkSource {
    /// A web page of the maker, as the printer's own script names it.
    Maker,
    /// The printer's own web server.
    Printer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Link {
    pub url: String,
    pub source: LinkSource,
}

impl Link {
    fn maker(url: String) -> Link {
        Link { url, source: LinkSource::Maker }
    }

    fn printer(url: String) -> Link {
        Link { url, source: LinkSource::Printer }
    }
}

/// Everything the app links to, for one printer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WebLinks {
    /// Where to order ink.
    pub ink: Link,
    /// The manual.
    pub help: Link,
    /// Help for the support code the links were asked for (`None` without a code).
    pub error: Option<Link>,
    /// The maker's support page.
    pub support: Link,
    /// The printer's own web page.
    pub printer_page: Link,
    /// The maintenance menu of the printer's web page (it asks for the printer's admin password).
    pub maintenance_page: Link,
    /// The printer offered its own links (`model.js` was read and had at least one).
    pub from_printer: bool,
    /// When the maker's links were saved (Unix time), if they come from the copy kept from an
    /// earlier read instead of from the printer just now: the printer serves them only while it
    /// is awake. `None` for a live read and for the fallbacks.
    pub saved_at: Option<u64>,
}

/// Put the links together from what was read. Pure: no I/O.
///
/// `model` is the parsed `model.js` (`None` when it could not be read), `pages` the
/// `remote_pages` of `GetCapability device`, `code` the support code to find help for, `lang`
/// the language of the app (the maker's addresses get its `LNG` parameter).
pub fn compose(
    host: &str,
    port: u16,
    model: Option<&ModelLinks>,
    pages: &BTreeMap<String, String>,
    code: Option<&str>,
    lang: Language,
) -> WebLinks {
    let own = format!("{}/", origin(host, port));
    let printer_page = Link::printer(own.clone());
    let model = model.map(|m| m.with_language(maker_language(lang))).unwrap_or_default();
    let pick = |u: &Option<String>| u.clone().map_or_else(|| printer_page.clone(), Link::maker);
    let code = code.map(str::trim).filter(|c| !c.is_empty());
    WebLinks {
        ink: pick(&model.ink),
        help: pick(&model.help),
        error: code.map(|c| model.error_url(c).map_or_else(|| printer_page.clone(), Link::maker)),
        support: pick(&model.support),
        maintenance_page: pages
            .get("maintenance")
            .and_then(|rel| page_url(host, port, rel))
            .map_or_else(|| printer_page.clone(), Link::printer),
        printer_page,
        from_printer: !model.is_empty(),
        saved_at: None,
    }
}

/// The value of the maker's `LNG` parameter for a language.
fn maker_language(lang: Language) -> &'static str {
    match lang {
        Language::En => "EN",
        Language::Ja => "JP",
    }
}

/// What the printer says about its web pages: the parsed `model.js` and the page links of its
/// device capability. Read once and kept by callers that compose links often.
#[derive(Debug, Clone, Default)]
pub struct WebSources {
    /// The language the printer was asked in (it words the maker's addresses for it).
    pub language: Language,
    pub model: Option<ModelLinks>,
    pub pages: BTreeMap<String, String>,
    /// `model` is the copy saved at this time (Unix), not a read made just now.
    pub saved_at: Option<u64>,
}

impl WebSources {
    /// Put the links together for a support code and a language.
    pub fn links(&self, host: &str, port: u16, code: Option<&str>, lang: Language) -> WebLinks {
        let mut l = compose(host, port, self.model.as_ref(), &self.pages, code, lang);
        if l.from_printer {
            l.saved_at = self.saved_at;
        }
        l
    }
}

impl Printer {
    /// Read what the printer says about its web pages (`GET /JS_MDL/model.js` and
    /// `GetCapability device`, both read-only). Never fails: what cannot be read is left out.
    pub fn web_sources(&self, lang: Language) -> WebSources {
        self.web_sources_kept(lang, None)
    }

    /// Like [`Printer::web_sources`], but with the copy of the printer's links kept in `cache`:
    /// the XK140 serves `model.js` only while it is awake (asleep it answers `404`), so a read
    /// that finds links keeps them, and a read that does not falls back to the kept copy, which
    /// is then marked with the time it was saved. Without a copy the links fall back to the
    /// printer's own page, as before.
    pub fn web_sources_kept(
        &self,
        lang: Language,
        cache: Option<&crate::localization::LocalizationCache>,
    ) -> WebSources {
        let opts = washi_net::WebOptions {
            accept_language: lang.code().to_string(),
            ..washi_net::WebOptions::default()
        };
        let live = washi_net::fetch_model_links(self.host(), self.chmp_port(), &opts)
            .ok()
            .filter(|m| !m.is_empty());
        let capability = self.device_capability().ok();
        let firmware = capability.as_ref().map(|c| c.firmver.clone()).filter(|f| !f.is_empty());
        let pages = capability.map(|c| c.remote_pages).unwrap_or_default();
        let (model, saved_at) = match live {
            Some(m) => {
                if let Some(c) = cache {
                    // Failing to keep a copy only costs the next sleeping read its links.
                    let _ = c.store_links(
                        &crate::localization::FetchedLinks { links: m.clone(), firmware },
                        crate::printer::unix_now(),
                    );
                }
                (Some(m), None)
            }
            None => match cache.and_then(|c| c.links()) {
                Some(k) => (Some(k.links), Some(k.fetched_at)),
                None => (None, None),
            },
        };
        WebSources { language: lang, model, pages, saved_at }
    }

    /// The web links for a support code (see [`Printer::web_sources`]); fallbacks for whatever the
    /// printer does not offer.
    pub fn web_links(&self, code: Option<&str>, lang: Language) -> WebLinks {
        self.web_sources(lang).links(self.host(), self.chmp_port(), code, lang)
    }

    /// Like [`Printer::web_links`], with the kept copy of the printer's links (see
    /// [`Printer::web_sources_kept`]).
    pub fn web_links_kept(
        &self,
        code: Option<&str>,
        lang: Language,
        cache: Option<&crate::localization::LocalizationCache>,
    ) -> WebLinks {
        self.web_sources_kept(lang, cache).links(self.host(), self.chmp_port(), code, lang)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../../washi-net/tests/fixtures/model-sample.js");

    fn pages() -> BTreeMap<String, String> {
        BTreeMap::from([("maintenance".to_string(), "index.html?page=PAGE_UTIL".to_string())])
    }

    #[test]
    fn links_come_from_the_printer_when_it_offers_them() {
        let model = ModelLinks::parse(SAMPLE);
        let l = compose("printer.local", 80, Some(&model), &pages(), Some("1300"), Language::En);
        assert!(l.from_printer);
        assert_eq!(l.ink.source, LinkSource::Maker);
        assert!(l.ink.url.contains("KIND=INK") && l.ink.url.contains("LNG=EN"), "{}", l.ink.url);
        assert!(l.help.url.contains("KIND=HELP"));
        let err = l.error.unwrap();
        assert_eq!(err.source, LinkSource::Maker);
        assert!(err.url.ends_with("&ERR=1300"), "{}", err.url);
        assert_eq!(l.support.url, "http://example.org/support/");
        assert_eq!(l.printer_page.url, "http://printer.local/");
        assert_eq!(l.maintenance_page.url, "http://printer.local/index.html?page=PAGE_UTIL");
        assert_eq!(l.maintenance_page.source, LinkSource::Printer);
    }

    #[test]
    fn the_language_of_the_app_goes_into_the_maker_links() {
        let model = ModelLinks::parse(SAMPLE);
        let l = compose("p.local", 80, Some(&model), &pages(), Some("1000"), Language::Ja);
        assert!(l.ink.url.contains("&LNG=JP"), "{}", l.ink.url);
        assert!(l.error.unwrap().url.contains("&LNG=JP"));
    }

    #[test]
    fn missing_links_fall_back_to_the_printers_own_page() {
        let l = compose("127.0.0.1", 18080, None, &BTreeMap::new(), Some("1300"), Language::En);
        assert!(!l.from_printer);
        let own = "http://127.0.0.1:18080/";
        for link in [&l.ink, &l.help, &l.support, &l.maintenance_page, l.error.as_ref().unwrap()] {
            assert_eq!(link.url, own);
            assert_eq!(link.source, LinkSource::Printer);
        }
        // Only the error help needs a code.
        assert_eq!(compose("p", 80, None, &pages(), None, Language::En).error, None);
        assert_eq!(compose("p", 80, None, &pages(), Some("  "), Language::En).error, None);
        // A script with some links: the others fall back one by one.
        let partial =
            ModelLinks { ink: Some("https://shop.example/x".into()), ..Default::default() };
        let l = compose("p.local", 80, Some(&partial), &pages(), Some("1300"), Language::En);
        assert_eq!(l.ink.source, LinkSource::Maker);
        assert_eq!(l.help.source, LinkSource::Printer);
        assert_eq!(l.error.unwrap().source, LinkSource::Printer, "no g_err_url");
    }

    #[test]
    fn an_unusable_maintenance_link_falls_back_too() {
        let bad = BTreeMap::from([("maintenance".to_string(), "http://evil.example/".to_string())]);
        let l = compose("p.local", 80, None, &bad, None, Language::En);
        assert_eq!(l.maintenance_page.url, "http://p.local/");
    }
}
