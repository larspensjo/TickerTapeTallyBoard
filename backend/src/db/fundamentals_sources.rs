use super::RepoError;
pub use crate::domain::fundamentals::{
    AttemptStatus, Classification, FundamentalsSource, LinkOrigin, LinkStatus,
};
use sqlx::{
    sqlite::{SqliteConnection, SqlitePool},
    Row,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkToken {
    pub generation: i64,
    pub symbol: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkDefinition {
    pub source: Option<FundamentalsSource>,
    pub symbol: Option<String>,
    pub security_type: Option<String>,
    pub classification: Classification,
    pub origin: LinkOrigin,
    pub status: LinkStatus,
    pub candidates: String,
}
impl LinkDefinition {
    pub fn pending() -> Self {
        Self {
            source: None,
            symbol: None,
            security_type: None,
            classification: Classification::Unknown,
            origin: LinkOrigin::Discovered,
            status: LinkStatus::Pending,
            candidates: "[]".to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceLink {
    pub instrument_id: i64,
    pub token: LinkToken,
    pub definition: LinkDefinition,
    pub last_attempt_at: Option<String>,
    pub last_attempt_status: Option<AttemptStatus>,
    pub last_attempt_reason: Option<String>,
    pub last_success_at: Option<String>,
    pub consecutive_empty: i64,
}

/// Calculation inputs exclude provider identity, discovery candidates and the
/// other metadata that only the source panel displays.
#[derive(Clone, Debug)]
pub struct FiguresState {
    pub classification: Classification,
    pub status: LinkStatus,
    pub last_attempt_status: Option<AttemptStatus>,
    pub last_success_at: Option<String>,
}
impl From<&SourceLink> for FiguresState {
    fn from(link: &SourceLink) -> Self {
        Self {
            classification: link.definition.classification,
            status: link.definition.status,
            last_attempt_status: link.last_attempt_status,
            last_success_at: link.last_success_at.clone(),
        }
    }
}

pub async fn figures_state(
    pool: &SqlitePool,
    instrument_id: i64,
) -> Result<Option<FiguresState>, RepoError> {
    sqlx::query("SELECT classification, status, last_attempt_status, last_success_at FROM instrument_fundamentals_sources WHERE instrument_id=?")
        .bind(instrument_id).fetch_optional(pool).await?.map(|row| {
            Ok(FiguresState {
                classification: code(&row.try_get::<String, _>("classification")?, Classification::from_db_str)?,
                status: code(&row.try_get::<String, _>("status")?, LinkStatus::from_db_str)?,
                last_attempt_status: row.try_get::<Option<String>, _>("last_attempt_status")?
                    .map(|s| code(&s, AttemptStatus::from_db_str)).transpose()?,
                last_success_at: row.try_get("last_success_at")?,
            })
        }).transpose()
}

pub(super) fn code<T>(raw: &str, parse: impl FnOnce(&str) -> Option<T>) -> Result<T, RepoError> {
    parse(raw).ok_or_else(|| RepoError::Decode(format!("unknown fundamentals code {raw:?}")))
}

fn decode(row: sqlx::sqlite::SqliteRow) -> Result<SourceLink, RepoError> {
    let symbol: Option<String> = row.try_get("provider_symbol")?;
    Ok(SourceLink {
        instrument_id: row.try_get("instrument_id")?,
        token: LinkToken {
            generation: row.try_get("generation")?,
            symbol: symbol.clone(),
        },
        definition: LinkDefinition {
            source: row
                .try_get::<Option<String>, _>("source")?
                .map(|s| code(&s, FundamentalsSource::from_db_str))
                .transpose()?,
            symbol,
            security_type: row.try_get("security_type")?,
            classification: code(
                &row.try_get::<String, _>("classification")?,
                Classification::from_db_str,
            )?,
            origin: code(
                &row.try_get::<String, _>("origin")?,
                LinkOrigin::from_db_str,
            )?,
            status: code(
                &row.try_get::<String, _>("status")?,
                LinkStatus::from_db_str,
            )?,
            candidates: row.try_get("candidates")?,
        },
        last_attempt_at: row.try_get("last_attempt_at")?,
        last_attempt_status: row
            .try_get::<Option<String>, _>("last_attempt_status")?
            .map(|s| code(&s, AttemptStatus::from_db_str))
            .transpose()?,
        last_attempt_reason: row.try_get("last_attempt_reason")?,
        last_success_at: row.try_get("last_success_at")?,
        consecutive_empty: row.try_get("consecutive_empty")?,
    })
}
pub async fn find<'e, E>(executor: E, instrument_id: i64) -> Result<Option<SourceLink>, RepoError>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    sqlx::query("SELECT * FROM instrument_fundamentals_sources WHERE instrument_id = ?")
        .bind(instrument_id)
        .fetch_optional(executor)
        .await?
        .map(decode)
        .transpose()
}

#[derive(Clone, Debug, PartialEq)]
pub enum GenerationWrite<T> {
    Applied(T),
    Superseded,
}

/// Call inside a leased BEGIN IMMEDIATE write, before any derived write. A
/// missing row is a distinct token, so discovery cannot race a manual link.
pub async fn matches(
    conn: &mut SqliteConnection,
    instrument_id: i64,
    expected: Option<&LinkToken>,
) -> Result<bool, RepoError> {
    Ok(find(&mut *conn, instrument_id)
        .await?
        .as_ref()
        .map(|link| &link.token)
        == expected)
}

pub(super) async fn replace(
    conn: &mut SqliteConnection,
    instrument_id: i64,
    definition: &LinkDefinition,
) -> Result<SourceLink, RepoError> {
    sqlx::query("INSERT INTO instrument_fundamentals_sources (instrument_id, source, provider_symbol, security_type, classification, origin, status, candidates, generation) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1) ON CONFLICT(instrument_id) DO UPDATE SET source=excluded.source, provider_symbol=excluded.provider_symbol, security_type=excluded.security_type, classification=excluded.classification, origin=excluded.origin, status=excluded.status, candidates=excluded.candidates, generation=generation+1, last_attempt_at=NULL, last_attempt_status=NULL, last_attempt_reason=NULL, last_success_at=NULL, consecutive_empty=0")
        .bind(instrument_id).bind(definition.source.map(FundamentalsSource::as_str)).bind(&definition.symbol).bind(&definition.security_type)
        .bind(definition.classification.as_str()).bind(definition.origin.as_str()).bind(definition.status.as_str()).bind(&definition.candidates).execute(&mut *conn).await?;
    find(&mut *conn, instrument_id)
        .await?
        .ok_or_else(|| RepoError::Decode("replacement link missing".to_owned()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviousLink {
    SameCompany,
    WrongCompany,
}
#[derive(Debug)]
pub enum CorrectionError {
    PreviousLinkRequired,
    Repo(RepoError),
}
impl From<RepoError> for CorrectionError {
    fn from(e: RepoError) -> Self {
        Self::Repo(e)
    }
}
impl From<sqlx::Error> for CorrectionError {
    fn from(e: sqlx::Error) -> Self {
        Self::Repo(e.into())
    }
}

/// Shared operation for replacement and return to discovery. The HTTP layer
/// supplies a manual definition, or `None` to return to automatic discovery.
pub async fn correct(
    pool: &SqlitePool,
    instrument_id: i64,
    replacement: Option<LinkDefinition>,
    previous: Option<PreviousLink>,
    at: &str,
) -> Result<SourceLink, CorrectionError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let has_values = super::fundamental_values::has_active_values(&mut *tx, instrument_id).await?;
    if has_values && previous.is_none() {
        return Err(CorrectionError::PreviousLinkRequired);
    }
    let definition = match replacement {
        Some(mut definition) => {
            definition.origin = LinkOrigin::Manual;
            definition
        }
        None => LinkDefinition::pending(),
    };
    let link = replace(&mut tx, instrument_id, &definition).await?;
    if previous == Some(PreviousLink::WrongCompany) {
        super::fundamental_values::set_aside(&mut tx, instrument_id, at).await?;
        super::report_announcements::set_aside(&mut tx, instrument_id, at).await?;
    }
    tx.commit().await?;
    Ok(link)
}
