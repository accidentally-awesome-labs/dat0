//! Heuristic routing classifier for the timing chip (design §6). If T0's S2
//! probe showed `duckdb_databases()`/EXPLAIN gives a reliable per-query signal,
//! replace the body with that; otherwise this string heuristic ships with the
//! documented limitation: it misses md tables referenced unqualified after a
//! `USE md` default-catalog switch.

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

pub fn classify_routing(sql: &str, attached_aliases: &[String]) -> Routing {
    let md_attached = attached_aliases.iter().any(|a| a == "md");
    if !md_attached {
        return Routing::Local;
    }
    let lower = sql.to_ascii_lowercase();
    // Qualified md reference: "md." not preceded by an identifier char.
    let has_md_ref = find_qualified(&lower, "md.");
    if !has_md_ref {
        return Routing::Local;
    }
    // Any FROM/JOIN target that is NOT md-qualified → also touches local.
    let touches_local = mentions_non_md_relation(&lower);
    if touches_local {
        Routing::Mixed
    } else {
        Routing::Md
    }
}

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

/// True if a FROM/JOIN clause references a relation that is not `md.`-qualified.
fn mentions_non_md_relation(lower: &str) -> bool {
    for kw in ["from ", "join "] {
        let mut from = 0;
        while let Some(idx) = lower[from..].find(kw) {
            let after = from + idx + kw.len();
            let rest = lower[after..].trim_start();
            let token: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
                .collect();
            if !token.is_empty() && !token.starts_with("md.") {
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

    #[test]
    fn no_md_alias_is_local() {
        assert_eq!(
            classify_routing("SELECT * FROM t", &["md".into()]),
            Routing::Local
        );
    }
    #[test]
    fn only_md_qualified_is_md() {
        assert_eq!(
            classify_routing("SELECT * FROM md.main.t", &["md".into()]),
            Routing::Md
        );
    }
    #[test]
    fn md_plus_local_is_mixed() {
        assert_eq!(
            classify_routing(
                "SELECT * FROM md.main.t JOIN local_t USING (id)",
                &["md".into()]
            ),
            Routing::Mixed
        );
    }
    #[test]
    fn md_not_attached_ignored() {
        assert_eq!(classify_routing("SELECT * FROM md.x", &[]), Routing::Local);
    }
}
