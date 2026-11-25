//! CottonCandy's browser logic, kept apart from the web engine so it can be tested on its own.
//!
//! The `cottoncandy` app crate puts this on Chromium (CEF). Nothing here touches the network by itself:
//! callers decide when to fetch filter lists, extensions or a model reply.

pub mod bookmarks_html;
pub mod extensions;
pub mod grace;
pub mod import;
pub mod omnibox;
pub mod paths;
pub mod settings;
pub mod shields;
pub mod store;

/// The name shown in window titles and the About page.
pub const APP_NAME: &str = "CottonCandy";
/// The scheme of CottonCandy's own pages (new tab, settings, history, Grace Chat).
pub const INTERNAL_SCHEME: &str = "cottoncandy";
