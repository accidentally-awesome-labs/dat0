//! Pure projection of Inspector cards onto the grid's display-only column
//! projection (P-projection). The Inspector profiles every base column (both
//! Whole-table and Current-view modes; projection is a no-op in
//! `compile_view_sql`). This module re-arranges those already-computed cards to
//! match the grid: visible columns in `column_view` order with renamed labels,
//! the rest under a "hidden" list, the internal surrogate always dropped. No
//! engine/GPUI dependency — fully unit-testable.
use dat0_engine::ColumnProfile;
use dat0_engine::transform::{ProjectionColumn, ROWID_COL};
use std::collections::HashSet;

/// The active grid tab's projection, supplied by `WorkspaceShell` only when the
/// Inspector targets that tab's table (else `None` → no-projection fallback).
#[derive(Debug, Clone)]
pub struct ProjectionContext {
    /// Grid-visible columns in display order (the folded `column_view`).
    pub visible: Vec<ProjectionColumn>,
    /// All non-surrogate base column names (to derive the hidden set).
    pub base_sources: Vec<String>,
}

/// One rendered card's identity: which profile column it maps to, the header
/// label to show, and the original name when the column was renamed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderCard {
    pub source: String,           // keys into `TableProfile.columns` by `.name`
    pub label: String,            // header text (renamed display label, or source)
    pub original: Option<String>, // Some(source) only when renamed (display != source)
}

/// The Inspector's cards split into grid-visible and hidden lists.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectedCards {
    pub visible: Vec<RenderCard>,
    pub hidden: Vec<RenderCard>,
}

/// Re-arrange `profile_cols` to match the grid projection `ctx`. Pure.
///
/// - The surrogate (`ROWID_COL`) is always dropped.
/// - `None` (Inspector not on the active grid table) → every non-surrogate
///   column visible in profile order, nothing hidden.
/// - `Some(ctx)` → `ctx.visible` order with renamed labels; columns in
///   `ctx.base_sources` but not visible become `hidden`; profile columns absent
///   from `base_sources` (and not the surrogate) are dropped.
pub fn project_cards(
    profile_cols: &[ColumnProfile],
    ctx: Option<&ProjectionContext>,
) -> ProjectedCards {
    let has = |name: &str| profile_cols.iter().any(|c| c.name == name);

    let Some(ctx) = ctx else {
        let visible = profile_cols
            .iter()
            .filter(|c| c.name != ROWID_COL)
            .map(|c| RenderCard {
                source: c.name.clone(),
                label: c.name.clone(),
                original: None,
            })
            .collect();
        return ProjectedCards {
            visible,
            hidden: Vec::new(),
        };
    };

    let mut visible_sources: HashSet<&str> = HashSet::new();
    let mut visible = Vec::new();
    for p in &ctx.visible {
        if p.source == ROWID_COL || !has(&p.source) {
            continue; // surrogate, or a projection col with no profile row (defensive)
        }
        visible_sources.insert(p.source.as_str());
        let original = (p.display != p.source).then(|| p.source.clone());
        visible.push(RenderCard {
            source: p.source.clone(),
            label: p.display.clone(),
            original,
        });
    }

    let hidden = ctx
        .base_sources
        .iter()
        .filter(|s| s.as_str() != ROWID_COL && !visible_sources.contains(s.as_str()) && has(s))
        .map(|s| RenderCard {
            source: s.clone(),
            label: s.clone(),
            original: None,
        })
        .collect();

    ProjectedCards { visible, hidden }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dat0_engine::ColumnProfile;
    use dat0_engine::transform::ROWID_COL;

    fn col(name: &str) -> ColumnProfile {
        ColumnProfile {
            name: name.into(),
            ty: "T".into(),
            null_pct: 0.0,
            approx_distinct: 0,
            count: 0,
            numeric: None,
            length: None,
        }
    }

    fn pc(source: &str, display: &str) -> ProjectionColumn {
        ProjectionColumn {
            source: source.into(),
            display: display.into(),
        }
    }

    // Profile carries 3 user columns + the surrogate (both modes do).
    fn profile() -> Vec<ColumnProfile> {
        vec![col("a"), col("b"), col("c"), col(ROWID_COL)]
    }

    #[test]
    fn no_projection_shows_all_user_columns_minus_surrogate() {
        let out = project_cards(&profile(), None);
        assert_eq!(
            out.visible
                .iter()
                .map(|c| c.label.clone())
                .collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        assert!(out.visible.iter().all(|c| c.original.is_none()));
        assert!(out.hidden.is_empty());
    }

    #[test]
    fn reorder_and_rename_follow_the_projection() {
        // Grid shows c, then b renamed to "Bee"; "a" is hidden.
        let ctx = ProjectionContext {
            visible: vec![pc("c", "c"), pc("b", "Bee")],
            base_sources: vec!["a".into(), "b".into(), "c".into()],
        };
        let out = project_cards(&profile(), Some(&ctx));

        assert_eq!(
            out.visible
                .iter()
                .map(|c| (c.source.clone(), c.label.clone(), c.original.clone()))
                .collect::<Vec<_>>(),
            vec![
                ("c".into(), "c".into(), None),
                ("b".into(), "Bee".into(), Some("b".into())),
            ]
        );
        // "a" is in base but not visible → hidden.
        assert_eq!(
            out.hidden
                .iter()
                .map(|c| c.source.clone())
                .collect::<Vec<_>>(),
            vec!["a"]
        );
        assert_eq!(out.hidden[0].label, "a");
        assert!(out.hidden[0].original.is_none());
    }

    #[test]
    fn surrogate_is_omitted_with_projection_too() {
        let ctx = ProjectionContext {
            visible: vec![pc("a", "a")],
            base_sources: vec!["a".into(), "b".into(), "c".into()],
        };
        let out = project_cards(&profile(), Some(&ctx));
        assert!(
            out.visible
                .iter()
                .chain(out.hidden.iter())
                .all(|c| c.source != ROWID_COL)
        );
        // b and c are hidden; surrogate not present anywhere.
        assert_eq!(
            out.hidden
                .iter()
                .map(|c| c.source.clone())
                .collect::<Vec<_>>(),
            vec!["b", "c"]
        );
    }

    #[test]
    fn visible_source_without_profile_column_is_skipped() {
        let ctx = ProjectionContext {
            visible: vec![pc("ghost", "ghost"), pc("a", "a")],
            base_sources: vec!["a".into()],
        };
        let out = project_cards(&profile(), Some(&ctx));
        assert_eq!(
            out.visible
                .iter()
                .map(|c| c.source.clone())
                .collect::<Vec<_>>(),
            vec!["a"]
        );
    }

    #[test]
    fn profile_column_absent_from_base_is_dropped() {
        // "c" exists in the profile but not in base_sources (and isn't the
        // surrogate) → it appears in neither list.
        let ctx = ProjectionContext {
            visible: vec![pc("a", "a")],
            base_sources: vec!["a".into(), "b".into()],
        };
        let out = project_cards(&profile(), Some(&ctx));
        let all: Vec<String> = out
            .visible
            .iter()
            .chain(out.hidden.iter())
            .map(|c| c.source.clone())
            .collect();
        assert!(!all.contains(&"c".to_string()));
        assert_eq!(
            out.hidden
                .iter()
                .map(|c| c.source.clone())
                .collect::<Vec<_>>(),
            vec!["b"]
        );
    }
}
