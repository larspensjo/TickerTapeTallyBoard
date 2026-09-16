/// The one policy emitted by the in-process HTTP bridge.
///
/// It intentionally uses no inline script allowance and no `data:` font source.
pub const VALUE: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; font-src 'self'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'self'; frame-ancestors 'none'";
