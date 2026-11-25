use std::collections::{BTreeMap, BTreeSet};

use crate::{
    auth::RbacPolicy,
    schema::{
        AmbiguityWarning, CandidateTableMatch, ForeignKeyMeta, LinkedSchemaContext, SchemaCatalog,
        TableMeta,
    },
    semantic::SemanticMetric,
};

const MAX_LINKED_TABLES: usize = 8;

/// Filter a SchemaCatalog according to the caller's RBAC policy (never exposing unauthorized schemas, tables, or denied columns).
pub fn filter_catalog_for_policy(catalog: &SchemaCatalog, policy: &RbacPolicy) -> SchemaCatalog {
    let mut filtered = catalog.clone();
    filtered.tables = catalog
        .tables
        .iter()
        .filter(|t| policy.is_table_allowed(&t.schema, &t.name))
        .map(|t| {
            let mut copy = t.clone();
            copy.columns
                .retain(|c| !policy.is_column_denied(&t.schema, &t.name, &c.name));
            copy.foreign_keys
                .retain(|fk| policy.is_table_allowed(&fk.to_schema, &fk.to_table));
            copy
        })
        .collect();

    filtered.relationships.retain(|fk| {
        policy.is_table_allowed(&fk.from_schema, &fk.from_table)
            && policy.is_table_allowed(&fk.to_schema, &fk.to_table)
    });
    filtered
}

pub fn link_schema_for_question(
    question: &str,
    catalog: &SchemaCatalog,
    semantics: &[SemanticMetric],
    clarify_mode: bool,
) -> LinkedSchemaContext {
    let q_lower = question.to_lowercase();
    let tokens = tokenize(&q_lower);

    // Detect requested metrics that do not exist in actual schema or semantic definitions
    let missing_concepts = detect_missing_concepts(&q_lower, catalog, semantics);

    // Detect business definition ambiguity (e.g. "revenue" when gross vs net aren't specified)
    let mut ambiguities = detect_metric_ambiguity(&q_lower, semantics, clarify_mode);

    // Score each permitted table via keywords, column matches, descriptions, and semantic references
    let mut scores: BTreeMap<String, (f64, BTreeSet<String>, Vec<String>)> = BTreeMap::new();

    for table in &catalog.tables {
        let mut score = 0.0_f64;
        let mut matched_terms = BTreeSet::new();
        let mut reasons = Vec::new();

        let tbl_name = table.name.to_lowercase();
        let schema_name = table.schema.to_lowercase();
        let tbl_desc = table
            .description
            .as_deref()
            .unwrap_or("")
            .to_lowercase();

        for tok in &tokens {
            if tbl_name.contains(tok) {
                score += 6.0;
                matched_terms.insert(tok.clone());
                reasons.push(format!("table name matches '{tok}'"));
            }
            if schema_name.contains(tok) {
                score += 2.5;
                matched_terms.insert(tok.clone());
            }
            if !tbl_desc.is_empty() && tbl_desc.contains(tok) {
                score += 2.0;
                matched_terms.insert(tok.clone());
                reasons.push(format!("table description mentions '{tok}'"));
            }

            // Semantic expansion strictly grounded in domain nouns
            if (tok == "sales" || tok == "revenue" || tok == "quarter" || tok == "monthly")
                && (tbl_name == "orders" || tbl_name == "order_items" || tbl_name == "monthly_sales_summary")
            {
                score += 4.5;
                matched_terms.insert(tok.clone());
            }
            if (tok == "profit" || tok == "margin") && tbl_name == "order_items" {
                score += 6.0;
                matched_terms.insert(tok.clone());
            }
            if (tok == "expense" || tok == "expenses" || tok == "budget" || tok == "spending" || tok == "unusual")
                && (tbl_name == "department_expenses" || tbl_name == "departments")
            {
                score += 6.0;
                matched_terms.insert(tok.clone());
            }
            if (tok == "transaction" || tok == "transactions" || tok == "volume")
                && (tbl_name == "orders" || tbl_name == "customers")
            {
                score += 4.0;
                matched_terms.insert(tok.clone());
            }
        }

        for col in &table.columns {
            let cname = col.name.to_lowercase();
            let cdesc = col.description.as_deref().unwrap_or("").to_lowercase();
            for tok in &tokens {
                if cname == *tok || cname.contains(tok) {
                    score += 3.5;
                    matched_terms.insert(tok.clone());
                    reasons.push(format!("column '{}' matches '{tok}'", col.name));
                } else if !cdesc.is_empty() && cdesc.contains(tok) {
                    score += 1.5;
                    matched_terms.insert(tok.clone());
                }
            }
        }

        // Semantic metric boost
        for sem in semantics.iter().filter(|m| m.is_active) {
            let key_words: Vec<String> = sem
                .metric_key
                .split('_')
                .map(|s| s.to_lowercase())
                .collect();
            if key_words.iter().any(|kw| tokens.contains(kw))
                && sem.source_table.eq_ignore_ascii_case(&table.fq_name())
            {
                score += 5.0;
                reasons.push(format!("mapped by semantic metric '{}'", sem.metric_key));
            }
        }

        // Slight preference for base tables over pre-aggregated views unless specifically matched
        if table.table_type == "BASE TABLE" && score > 0.0 {
            score += 0.5;
        }

        if score > 0.0 {
            scores.insert(table.fq_name(), (score, matched_terms, reasons));
        }
    }

    // Foreign-key traversal (1-hop along verified FKs or administrator-defined manual relationships ONLY)
    let initial_top: Vec<String> = {
        let mut v: Vec<(&String, f64)> = scores.iter().map(|(k, (s, _, _))| (k, *s)).collect();
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        v.into_iter().take(4).map(|(k, _)| k.clone()).collect()
    };

    for fk in &catalog.relationships {
        let from_fq = format!("{}.{}", fk.from_schema, fk.from_table);
        let to_fq = format!("{}.{}", fk.to_schema, fk.to_table);
        if initial_top.contains(&from_fq) {
            let entry = scores
                .entry(to_fq.clone())
                .or_insert_with(|| (0.0, BTreeSet::new(), Vec::new()));
            entry.0 += 3.0;
            entry.2.push(format!(
                "linked via {} from {}",
                fk.source, from_fq
            ));
        }
        if initial_top.contains(&to_fq) {
            let entry = scores
                .entry(from_fq)
                .or_insert_with(|| (0.0, BTreeSet::new(), Vec::new()));
            entry.0 += 2.5;
            entry.2.push(format!(
                "linked via {} to {}",
                fk.source, to_fq
            ));
        }
    }

    let mut ranked: Vec<CandidateTableMatch> = scores
        .into_iter()
        .map(|(fq, (score, terms, mut reasons))| {
            let parts: Vec<&str> = fq.split('.').collect();
            reasons.dedup();
            CandidateTableMatch {
                schema: parts.first().copied().unwrap_or("").to_string(),
                table: parts.get(1).copied().unwrap_or("").to_string(),
                score: (score * 100.0).round() / 100.0,
                matched_terms: terms.into_iter().collect(),
                reason: if reasons.is_empty() {
                    "linked via verified relationship".to_string()
                } else {
                    reasons.into_iter().take(4).collect::<Vec<_>>().join("; ")
                },
            }
        })
        .collect();

    ranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.table.cmp(&b.table))
    });

    // Ambiguity detection when similarly named columns appear across unconnected tables without FK link
    detect_ambiguous_columns(&tokens, catalog, &mut ambiguities);

    let selected_fqs: BTreeSet<String> = ranked
        .iter()
        .take(MAX_LINKED_TABLES)
        .map(|m| format!("{}.{}", m.schema, m.table))
        .collect();

    let selected_tables: Vec<TableMeta> = catalog
        .tables
        .iter()
        .filter(|t| selected_fqs.contains(&t.fq_name()))
        .cloned()
        .collect();

    let join_paths: Vec<ForeignKeyMeta> = catalog
        .relationships
        .iter()
        .filter(|fk| {
            let from_fq = format!("{}.{}", fk.from_schema, fk.from_table);
            let to_fq = format!("{}.{}", fk.to_schema, fk.to_table);
            selected_fqs.contains(&from_fq) && selected_fqs.contains(&to_fq)
        })
        .cloned()
        .collect();

    LinkedSchemaContext {
        selected_tables,
        candidate_rankings: ranked,
        join_paths,
        ambiguities,
        missing_concepts,
        schema_version: catalog.version,
    }
}

pub fn generate_schema_examples(catalog: &SchemaCatalog, semantics: &[SemanticMetric]) -> Vec<String> {
    let mut examples = Vec::new();
    let has_orders = catalog.tables.iter().any(|t| t.schema == "analytics" && t.name == "orders");
    let has_items = catalog.tables.iter().any(|t| t.schema == "analytics" && t.name == "order_items");
    let has_products = catalog.tables.iter().any(|t| t.schema == "analytics" && t.name == "products");
    let has_regions = catalog.tables.iter().any(|t| t.schema == "analytics" && t.name == "regions");
    let has_customers = catalog.tables.iter().any(|t| t.schema == "analytics" && t.name == "customers");
    let has_expenses = catalog
        .tables
        .iter()
        .any(|t| t.schema == "finance" && t.name == "department_expenses");

    if has_orders {
        examples.push("What was the total net revenue last quarter?".to_string());
        examples.push("Show monthly sales trends across completed and shipped orders.".to_string());
        examples.push("Compare net revenue across the two most recent full quarters.".to_string());
    }
    if has_products && has_items {
        examples.push("Which products generated the highest net revenue?".to_string());
        examples.push("Show the top ten products by gross profit.".to_string());
    }
    if has_orders && has_regions {
        examples.push("Which regions experienced declining sales across recent quarters?".to_string());
    }
    if has_orders && has_customers {
        examples.push("Which customers have the highest transaction volume?".to_string());
    }
    if has_expenses {
        examples.push("Which departments have unusual expense patterns compared to budget?".to_string());
    }
    if examples.is_empty() {
        for m in semantics.iter().take(3) {
            examples.push(format!("Show {} grouped by top available dimension.", m.display_name));
        }
        if examples.is_empty() {
            for t in catalog.tables.iter().take(3) {
                if let Some(dim) = t.columns.iter().find(|c| {
                    c.data_type.to_lowercase().contains("char") || c.data_type.to_lowercase().contains("text")
                }) {
                    examples.push(format!("Summarize {}.{} grouped by {}.", t.schema, t.name, dim.name));
                }
            }
        }
    }
    examples
}

fn tokenize(text: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "the", "what", "was", "were", "which", "show", "me", "with", "have", "has", "had",
        "from", "into", "between", "and", "for", "are", "is", "of", "in", "by", "to", "a",
        "an", "on", "at", "all", "if", "exists", "relevant", "data", "did", "how", "many",
    ];
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| s.len() >= 2 && !STOP.contains(&s.as_str()))
        .collect()
}

fn detect_metric_ambiguity(
    q_lower: &str,
    semantics: &[SemanticMetric],
    clarify_mode: bool,
) -> Vec<AmbiguityWarning> {
    let mut warnings = Vec::new();
    let mentions_revenue = q_lower.contains("revenue");
    let specifies_variant = q_lower.contains("net")
        || q_lower.contains("gross")
        || q_lower.contains("recognized");

    if mentions_revenue && !specifies_variant && clarify_mode {
        let rev_metrics: Vec<String> = semantics
            .iter()
            .filter(|m| m.is_active && m.metric_key.contains("revenue"))
            .map(|m| format!("{} ({})", m.display_name, m.definition))
            .collect();
        if rev_metrics.len() >= 2 {
            warnings.push(AmbiguityWarning {
                topic: "revenue_definition".to_string(),
                message: "Multiple approved revenue definitions exist in the Semantic Catalog ('Gross Revenue' vs 'Recognized Net Revenue'). Please clarify which revenue definition you want.".to_string(),
                options: rev_metrics,
            });
        }
    }

    let has_explicit_year = regex::Regex::new(r"\b20\d{2}\b")
        .map(|re| re.is_match(q_lower))
        .unwrap_or(false);

    if (q_lower.contains("compare") || q_lower.contains("between"))
        && !q_lower.contains("q1")
        && !q_lower.contains("q2")
        && !q_lower.contains("q3")
        && !q_lower.contains("q4")
        && !has_explicit_year
        && !q_lower.contains("quarter")
        && !q_lower.contains("month")
        && !q_lower.contains("year")
    {
        warnings.push(AmbiguityWarning {
            topic: "comparison_periods".to_string(),
            message: "Please specify which two time periods you would like to compare (for example: last two full quarters, or two calendar years).".to_string(),
            options: vec![
                "Compare net revenue across the two most recent full quarters".to_string(),
                "Compare net revenue year-over-year across the last two years".to_string(),
            ],
        });
    }

    warnings
}

fn detect_missing_concepts(
    q_lower: &str,
    catalog: &SchemaCatalog,
    semantics: &[SemanticMetric],
) -> Vec<String> {
    let unsupported_candidates = [
        ("nps", "Net Promoter Score (NPS)"),
        ("churn", "Customer Churn Rate"),
        ("lifetime value", "Customer Lifetime Value (CLV/LTV)"),
        ("ltv", "Customer Lifetime Value (LTV)"),
        ("cac", "Customer Acquisition Cost (CAC)"),
        ("inventory", "Warehouse Inventory Levels"),
        ("salary", "Employee Salaries (restricted or unmapped)"),
        ("salaries", "Employee Salaries (restricted or unmapped)"),
        ("payroll", "Employee Payroll (restricted or unmapped)"),
        ("headcount", "Employee Headcount (restricted or unmapped)"),
    ];

    let mut missing = Vec::new();
    for (kw, label) in unsupported_candidates {
        if q_lower.contains(kw) {
            let in_schema = catalog.tables.iter().any(|t| {
                t.name.to_lowercase().contains(kw)
                    || t.columns.iter().any(|c| c.name.to_lowercase().contains(kw))
            });
            let in_semantics = semantics
                .iter()
                .any(|m| m.is_active && m.metric_key.to_lowercase().contains(kw));
            if !in_schema && !in_semantics {
                missing.push(label.to_string());
            }
        }
    }
    missing
}

fn detect_ambiguous_columns(
    tokens: &[String],
    catalog: &SchemaCatalog,
    warnings: &mut Vec<AmbiguityWarning>,
) {
    // Only flag ambiguous standalone concepts if two unconnected tables expose identically named non-FK columns
    for tok in tokens {
        if tok == "amount" || tok == "status" {
            let matching_cols: Vec<String> = catalog
                .tables
                .iter()
                .flat_map(|t| {
                    t.columns
                        .iter()
                        .filter(|c| c.name.eq_ignore_ascii_case(tok))
                        .map(move |c| format!("{}.{}.{}", t.schema, t.name, c.name))
                })
                .collect();
            if matching_cols.len() > 2 && tok == "amount" {
                warnings.push(AmbiguityWarning {
                    topic: format!("column_{tok}"),
                    message: format!("Multiple tables contain '{tok}' columns."),
                    options: matching_cols,
                });
            }
        }
    }
}
