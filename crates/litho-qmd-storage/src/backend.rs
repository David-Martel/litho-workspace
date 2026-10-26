#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QmdBackendKind {
    Postgres,
    Sqlite,
}
