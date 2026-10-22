//! Static files the pages link to: the site's bundles, the framework's own
//! files and the favicon, served by the front server like files on disk.

use minijinja::context;

use crate::persona::Favicon;
use crate::render::doc::Doc;
use crate::render::scope::{Scope, ser};
use crate::render::static_origin;
use crate::world::icon;

pub fn render(scope: &Scope) -> Doc {
    let p = scope.persona();
    let origin = static_origin(scope, "asset");
    let path = scope.req.path.as_str();
    if path == "/favicon.ico" {
        let body = match p.assets.favicon {
            Favicon::Own => icon::favicon(&p.assets.brand),
            Favicon::Empty | Favicon::None => Vec::new(),
        };
        return Doc::new(origin, None, body);
    }
    let Some((name, file)) = p.assets.files.iter().find(|(_, f)| f.path == path) else {
        return Doc::new(origin, None, Vec::new());
    };
    let body = scope.render_or_empty(file.body, context! { name => name, file => ser(file), brand => ser(&p.assets.brand) });
    Doc::new(origin, None, body)
}
