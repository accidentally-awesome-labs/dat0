//! Heuristic routing classifier for the timing chip (design §6).
//!
//! MotherDuck workspace mode attaches the account's databases under their REAL
//! names (e.g. `sample_data`, `my_db`) — there is no single `md` alias. So a
//! query "touches MotherDuck" iff it references one of the currently-attached MD
//! database names, qualified (`<db>.…`). Documented limitation of the string
//! heuristic: it misses MD tables referenced unqualified after a `USE <md_db>`
//! default-catalog switch.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Routing {
    Local,
    Md,
    Mixed,
}

impl Routing {
    /// i18n key suffix for the timing chip (`sql.local` / `sql.md` / `sql.mixed`).
    pub fn i18n_key(self) -> &'static str {
        match self {
            Routing::Local => "sql.local",
            Routing::Md => "sql.md",
            Routing::Mixed => "sql.mixed",
        }
    }
}

/// Classify a query against the set of currently-attached MotherDuck database
/// names. `Local` if none attached or no MD db referenced; `Md` if every
/// FROM/JOIN relation is MD-qualified; `Mixed` if it also touches a non-MD
/// relation.
pub fn classify_routing(sql: &str, md_databases: &[String]) -> Routing {
    if md_databases.is_empty() {
        return Routing::Local;
    }
    let lower = sql.to_ascii_lowercase();
    let md_lc: Vec<String> = md_databases
        .iter()
        .map(|d| d.to_ascii_lowercase())
        .collect();

    // Any qualified reference to an attached MD database (`<db>.`)?
    let has_md_ref = md_lc
        .iter()
        .any(|db| find_qualified(&lower, &format!("{db}.")));
    if !has_md_ref {
        return Routing::Local;
    }
    // Any FROM/JOIN target that is NOT MD-qualified → also touches local.
    let touches_local = mentions_non_md_relation(&lower, &md_lc);
    if touches_local {
        Routing::Mixed
    } else {
        Routing::Md
    }
}

/// True if `needle` occurs in `lower` not immediately preceded by an identifier
/// character (so `md.` matches but `xmd.` does not). `needle` is lowercase.
fn find_qualified(lower: &str, needle: &str) -> bool {
    let mut from = 0;
    while let Some(idx) = lower[from..].find(needle) {
        let abs = from + idx;
        let prev_ok = abs == 0
            || !lower.as_bytes()[abs - 1].is_ascii_alphanumeric()
                && lower.as_bytes()[abs - 1] != b'_';
        if prev_ok {
            return true;
        }
        from = abs + needle.len();
    }
    false
}

/// True if a FROM/JOIN clause references a relation not prefixed by any attached
/// MD database name (`<md_db>.`). `md_dbs` entries are lowercase.
fn mentions_non_md_relation(lower: &str, md_dbs: &[String]) -> bool {
    for kw in ["from ", "join "] {
        let mut from = 0;
        while let Some(idx) = lower[from..].find(kw) {
            let after = from + idx + kw.len();
            let rest = lower[after..].trim_start();
            let token: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
                .collect();
            if !token.is_empty() && !md_dbs.iter().any(|db| token.starts_with(&format!("{db}."))) {
                return true;
            }
            from = after;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn md() -> Vec<String> {
        vec!["sample_data".into(), "my_db".into()]
    }

    #[test]
    fn no_md_reference_is_local() {
        assert_eq!(classify_routing("SELECT * FROM t", &md()), Routing::Local);
    }
    #[test]
    fn only_md_qualified_is_md() {
        assert_eq!(
            classify_routing("SELECT * FROM sample_data.main.t", &md()),
            Routing::Md
        );
    }
    #[test]
    fn md_plus_local_is_mixed() {
        assert_eq!(
            classify_routing(
                "SELECT * FROM sample_data.main.t JOIN local_t USING (id)",
                &md()
            ),
            Routing::Mixed
        );
    }
    #[test]
    fn second_md_db_also_detected() {
        assert_eq!(
            classify_routing("SELECT * FROM my_db.main.t", &md()),
            Routing::Md
        );
    }
    #[test]
    fn md_not_attached_ignored() {
        assert_eq!(
            classify_routing("SELECT * FROM sample_data.x", &[]),
            Routing::Local
        );
    }
}
