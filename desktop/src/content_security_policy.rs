/// The one policy emitted by the in-process HTTP bridge.
///
/// It intentionally uses no inline script allowance, no `data:` font source, and
/// no `data:` image source. Every image the application loads is a served file.
pub const VALUE: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; font-src 'self'; img-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'self'; frame-ancestors 'none'";
