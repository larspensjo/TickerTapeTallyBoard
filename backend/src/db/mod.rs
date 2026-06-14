mod pool;

pub mod instruments;
pub mod transactions;

#[cfg(test)]
pub mod testing;

pub use pool::connect;

/// Errors a repository can surface: a SQL/driver failure, or a failure to decode
/// stored data back into domain types (an internal invariant violation).
#[derive(Debug)]
pub enum RepoError {
    Sqlx(sqlx::Error),
    Decode(String),
}

impl std::fmt::Display for RepoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlx(error) => write!(f, "database error: {error}"),
            Self::Decode(message) => write!(f, "decode error: {message}"),
        }
    }
}

impl std::error::Error for RepoError {}

impl From<sqlx::Error> for RepoError {
    fn from(error: sqlx::Error) -> Self {
        Self::Sqlx(error)
    }
}
