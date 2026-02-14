use std::collections::BTreeMap;

use crate::catalog::read_catalog;
use crate::data::{self, Value};
use crate::file::{FileError, PageReader};
use crate::format::ObjectType;
use crate::table;

// ---------------------------------------------------------------------------
// Attribute constants
// ---------------------------------------------------------------------------

const ATTR_TYPE: u8 = 1;
const ATTR_PARAMETER: u8 = 2;
const ATTR_FLAG: u8 = 3;
const ATTR_TABLE: u8 = 5;
const ATTR_COLUMN: u8 = 6;
const ATTR_JOIN: u8 = 7;
const ATTR_WHERE: u8 = 8;
const ATTR_GROUPBY: u8 = 9;
const ATTR_HAVING: u8 = 10;
const ATTR_ORDERBY: u8 = 11;

// SELECT flag bits (from FLAG attribute row)
const SELECT_STAR: i16 = 0x01;
const DISTINCT: i16 = 0x02;
const OWNER_ACCESS: i16 = 0x04;
const DISTINCT_ROW: i16 = 0x08;
const TOP: i16 = 0x10;
const PERCENT: i16 = 0x20;

const UNION_FLAG: i16 = 0x02;
const APPEND_VALUE_FLAG: i16 = -0x8000; // 0x8000 as i16
const CROSSTAB_PIVOT_FLAG: i16 = 0x01;
const CROSSTAB_NORMAL_FLAG: i16 = 0x02;

const UNION_PART1: &str = "X7YZ_____1";
const UNION_PART2: &str = "X7YZ_____2";

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Query type, determined by the Flag field of the TYPE attribute row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryType {
    Select = 1,
    MakeTable = 2,
    Append = 3,
    Update = 4,
    Delete = 5,
    Crosstab = 6,
    Ddl = 7,
    Passthrough = 8,
    Union = 9,
}

impl QueryType {
    fn from_flag(flag: i16) -> Option<Self> {
        match flag {
            1 => Some(Self::Select),
            2 => Some(Self::MakeTable),
            3 => Some(Self::Append),
            4 => Some(Self::Update),
            5 => Some(Self::Delete),
            6 => Some(Self::Crosstab),
            7 => Some(Self::Ddl),
            8 => Some(Self::Passthrough),
            9 => Some(Self::Union),
            _ => None,
        }
    }
}

/// A parsed query definition from MSysQueries.
#[derive(Debug, Clone)]
pub struct QueryDef {
    pub name: String,
    pub query_type: QueryType,
    rows: Vec<QueryRow>,
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct QueryRow {
    attribute: u8,
    expression: Option<String>,
    name1: Option<String>,
    name2: Option<String>,
    flag: Option<i16>,
    extra: Option<i32>,
}

// ---------------------------------------------------------------------------
// read_queries
// ---------------------------------------------------------------------------

/// Read all query definitions from the MSysQueries system table.
///
/// Returns an empty `Vec` if the table does not exist.
pub fn read_queries(reader: &mut PageReader) -> Result<Vec<QueryDef>, FileError> {
    let catalog = read_catalog(reader)?;

    let queries_entry = catalog.iter().find(|e| {
        e.name == "MSysQueries"
            && matches!(e.object_type, ObjectType::Table | ObjectType::SystemTable)
    });
    let queries_page = match queries_entry {
        Some(e) => e.table_page,
        None => return Ok(Vec::new()),
    };

    let tdef = table::read_table_def(reader, "MSysQueries", queries_page)?;
    let result = data::read_table_rows(reader, &tdef)?;

    // Locate column indices
    let mut object_id_idx = None;
    let mut attribute_idx = None;
    let mut order_idx = None;
    let mut name1_idx = None;
    let mut name2_idx = None;
    let mut expression_idx = None;
    let mut flag_idx = None;
    let mut extra_idx = None;

    for (i, col) in tdef.columns.iter().enumerate() {
        match col.name.as_str() {
            "ObjectId" => object_id_idx = Some(i),
            "Attribute" => attribute_idx = Some(i),
            "Order" => order_idx = Some(i),
            "Name1" => name1_idx = Some(i),
            "Name2" => name2_idx = Some(i),
            "Expression" => expression_idx = Some(i),
            "Flag" => flag_idx = Some(i),
            "LvExtra" => extra_idx = Some(i),
            _ => {}
        }
    }

    let object_id_idx = object_id_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysQueries missing ObjectId column",
    })?;
    let attribute_idx = attribute_idx.ok_or(FileError::InvalidTableDef {
        reason: "MSysQueries missing Attribute column",
    })?;

    // Group rows by ObjectId
    struct RawRow {
        attribute: u8,
        order: Vec<u8>,
        name1: Option<String>,
        name2: Option<String>,
        expression: Option<String>,
        flag: Option<i16>,
        extra: Option<i32>,
    }

    let mut groups: BTreeMap<i32, Vec<RawRow>> = BTreeMap::new();

    for row in &result.rows {
        let object_id = match row.get(object_id_idx) {
            Some(Value::Long(v)) => *v,
            _ => continue,
        };
        let attribute = match row.get(attribute_idx) {
            Some(Value::Byte(v)) => *v,
            _ => continue,
        };
        let order = match order_idx.and_then(|i| row.get(i)) {
            Some(Value::Binary(b)) => b.clone(),
            _ => Vec::new(),
        };
        let name1 = match name1_idx.and_then(|i| row.get(i)) {
            Some(Value::Text(s)) if !s.is_empty() => Some(s.clone()),
            _ => None,
        };
        let name2 = match name2_idx.and_then(|i| row.get(i)) {
            Some(Value::Text(s)) if !s.is_empty() => Some(s.clone()),
            _ => None,
        };
        let expression = match expression_idx.and_then(|i| row.get(i)) {
            Some(Value::Text(s)) if !s.is_empty() => {
                let trimmed = s.trim_end_matches('\0');
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }
            _ => None,
        };
        let flag = match flag_idx.and_then(|i| row.get(i)) {
            Some(Value::Int(v)) => Some(*v),
            _ => None,
        };
        let extra = match extra_idx.and_then(|i| row.get(i)) {
            Some(Value::Long(v)) => Some(*v),
            _ => None,
        };

        groups.entry(object_id).or_default().push(RawRow {
            attribute,
            order,
            name1,
            name2,
            expression,
            flag,
            extra,
        });
    }

    // Build query name map from catalog
    let query_name_map: BTreeMap<u32, String> = catalog
        .iter()
        .filter(|e| e.object_type == ObjectType::Query)
        .map(|e| (e.table_page, e.name.clone()))
        .collect();

    let mut queries = Vec::new();
    for (object_id, mut raw_rows) in groups {
        // Sort rows by Order field
        raw_rows.sort_by(|a, b| a.order.cmp(&b.order));

        // Find TYPE row to determine query type
        let type_row = raw_rows.iter().find(|r| r.attribute == ATTR_TYPE);
        let query_type = match type_row
            .and_then(|r| r.flag)
            .and_then(QueryType::from_flag)
        {
            Some(qt) => qt,
            None => continue,
        };

        // Match to catalog name
        let page_key = (object_id as u32) & 0x00FF_FFFF;
        let name = match query_name_map.get(&page_key) {
            Some(n) => n.clone(),
            None => continue,
        };

        let rows: Vec<QueryRow> = raw_rows
            .into_iter()
            .map(|r| QueryRow {
                attribute: r.attribute,
                expression: r.expression,
                name1: r.name1,
                name2: r.name2,
                flag: r.flag,
                extra: r.extra,
            })
            .collect();

        queries.push(QueryDef {
            name,
            query_type,
            rows,
        });
    }

    Ok(queries)
}

// ---------------------------------------------------------------------------
// query_to_sql — SQL restoration
// ---------------------------------------------------------------------------

/// Restore the SQL string for a query definition.
pub fn query_to_sql(qdef: &QueryDef) -> String {
    let mut builder = String::new();
    let supports_standard = !matches!(
        qdef.query_type,
        QueryType::Passthrough | QueryType::Ddl
    );

    if supports_standard {
        let params = format_parameters(&qdef.rows);
        if !params.is_empty() {
            builder.push_str("PARAMETERS ");
            builder.push_str(&params);
            builder.push_str(";\n");
        }
    }

    match qdef.query_type {
        QueryType::Select => sql_select(&mut builder, &qdef.rows),
        QueryType::Delete => sql_delete(&mut builder, &qdef.rows),
        QueryType::Update => sql_update(&mut builder, &qdef.rows),
        QueryType::Append => sql_append(&mut builder, &qdef.rows),
        QueryType::MakeTable => sql_make_table(&mut builder, &qdef.rows),
        QueryType::Crosstab => sql_crosstab(&mut builder, &qdef.rows),
        QueryType::Union => sql_union(&mut builder, &qdef.rows),
        QueryType::Passthrough => sql_passthrough(&mut builder, &qdef.rows),
        QueryType::Ddl => sql_ddl(&mut builder, &qdef.rows),
    }

    if supports_standard {
        if has_flag(&qdef.rows, OWNER_ACCESS) {
            builder.push_str("\nWITH OWNERACCESS OPTION");
        }
        builder.push(';');
    }

    builder
}

// ---------------------------------------------------------------------------
// Row access helpers
// ---------------------------------------------------------------------------

fn rows_by_attr<'a>(rows: &'a [QueryRow], attr: u8) -> Vec<&'a QueryRow> {
    rows.iter().filter(|r| r.attribute == attr).collect()
}

fn flag_row(rows: &[QueryRow]) -> Option<&QueryRow> {
    rows.iter().find(|r| r.attribute == ATTR_FLAG)
}

fn has_flag(rows: &[QueryRow], mask: i16) -> bool {
    flag_row(rows)
        .and_then(|r| r.flag)
        .map(|f| (f & mask) != 0)
        .unwrap_or(false)
}

fn type_row(rows: &[QueryRow]) -> Option<&QueryRow> {
    rows.iter().find(|r| r.attribute == ATTR_TYPE)
}

fn where_expr(rows: &[QueryRow]) -> Option<&str> {
    rows.iter()
        .find(|r| r.attribute == ATTR_WHERE)
        .and_then(|r| r.expression.as_deref())
}

fn having_expr(rows: &[QueryRow]) -> Option<&str> {
    rows.iter()
        .find(|r| r.attribute == ATTR_HAVING)
        .and_then(|r| r.expression.as_deref())
}

// ---------------------------------------------------------------------------
// Identifier quoting
// ---------------------------------------------------------------------------

fn needs_quoting(s: &str) -> bool {
    s.chars().any(|c| !c.is_ascii_alphanumeric() && c != '_')
}

fn is_quoted(s: &str) -> bool {
    s.len() >= 2 && s.starts_with('[') && s.ends_with(']')
}

fn to_quoted_expr(s: &str) -> String {
    if is_quoted(s) {
        s.to_string()
    } else {
        format!("[{s}]")
    }
}

/// Quote an expression, splitting on '.' for identifiers.
fn to_optional_quoted(full_expr: &str, is_identifier: bool) -> String {
    if is_identifier {
        full_expr
            .split('.')
            .map(|part| {
                if needs_quoting(part) {
                    to_quoted_expr(part)
                } else {
                    part.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(".")
    } else if needs_quoting(full_expr) {
        to_quoted_expr(full_expr)
    } else {
        full_expr.to_string()
    }
}

fn to_alias(alias: Option<&str>) -> String {
    match alias {
        Some(a) => format!(" AS {}", to_optional_quoted(a, false)),
        None => String::new(),
    }
}

// ---------------------------------------------------------------------------
// PARAMETERS clause
// ---------------------------------------------------------------------------

fn param_type_name(flag: i16) -> Option<&'static str> {
    match flag {
        0 => Some("Value"),
        1 => Some("Bit"),
        10 => Some("Text"),
        2 => Some("Byte"),
        3 => Some("Short"),
        4 => Some("Long"),
        5 => Some("Currency"),
        6 => Some("IEEESingle"),
        7 => Some("IEEEDouble"),
        8 => Some("DateTime"),
        9 => Some("Binary"),
        11 => Some("LongBinary"),
        15 => Some("Guid"),
        _ => None,
    }
}

fn format_parameters(rows: &[QueryRow]) -> String {
    let param_rows = rows_by_attr(rows, ATTR_PARAMETER);
    let parts: Vec<String> = param_rows
        .iter()
        .filter_map(|r| {
            let name1 = r.name1.as_deref()?;
            let flag = r.flag?;
            let type_name = param_type_name(flag)?;
            let mut s = format!("{name1} {type_name}");
            // TEXT type with extra size
            if flag == 10 {
                if let Some(extra) = r.extra {
                    if extra > 0 {
                        s.push_str(&format!("({extra})"));
                    }
                }
            }
            Some(s)
        })
        .collect();
    parts.join(", ")
}

// ---------------------------------------------------------------------------
// SELECT type modifier (DISTINCT, TOP, etc.)
// ---------------------------------------------------------------------------

fn get_select_type(rows: &[QueryRow]) -> String {
    if has_flag(rows, DISTINCT) {
        return "DISTINCT".to_string();
    }
    if has_flag(rows, DISTINCT_ROW) {
        return "DISTINCTROW".to_string();
    }
    if has_flag(rows, TOP) {
        let n = flag_row(rows).and_then(|r| r.name1.as_deref()).unwrap_or("");
        let mut s = format!("TOP {n}");
        if has_flag(rows, PERCENT) {
            s.push_str(" PERCENT");
        }
        return s;
    }
    String::new()
}

// ---------------------------------------------------------------------------
// SELECT columns
// ---------------------------------------------------------------------------

fn get_select_columns(rows: &[QueryRow], filter: impl Fn(&QueryRow) -> bool) -> String {
    let column_rows = rows_by_attr(rows, ATTR_COLUMN);
    let mut parts: Vec<String> = column_rows
        .iter()
        .filter(|r| filter(r))
        .filter_map(|r| {
            let expr = r.expression.as_deref()?;
            let mut s = expr.to_string();
            s.push_str(&to_alias(r.name1.as_deref()));
            Some(s)
        })
        .collect();
    if has_flag(rows, SELECT_STAR) {
        parts.push("*".to_string());
    }
    parts.join(", ")
}

// ---------------------------------------------------------------------------
// GROUP BY
// ---------------------------------------------------------------------------

fn get_groupings(rows: &[QueryRow], filter: impl Fn(&QueryRow) -> bool) -> String {
    let gb_rows = rows_by_attr(rows, ATTR_GROUPBY);
    let parts: Vec<&str> = gb_rows
        .iter()
        .filter(|r| filter(r))
        .filter_map(|r| r.expression.as_deref())
        .collect();
    parts.join(", ")
}

// ---------------------------------------------------------------------------
// ORDER BY
// ---------------------------------------------------------------------------

fn get_orderings(rows: &[QueryRow]) -> String {
    let ob_rows = rows_by_attr(rows, ATTR_ORDERBY);
    let parts: Vec<String> = ob_rows
        .iter()
        .filter_map(|r| {
            let expr = r.expression.as_deref()?;
            let mut s = expr.to_string();
            if r.name1
                .as_deref()
                .map(|n| n.eq_ignore_ascii_case("D"))
                .unwrap_or(false)
            {
                s.push_str(" DESC");
            }
            Some(s)
        })
        .collect();
    parts.join(", ")
}

// ---------------------------------------------------------------------------
// FROM clause + JOIN building
// ---------------------------------------------------------------------------

enum TableSource {
    Simple {
        name: String,
        expr: String,
    },
    Join {
        from: Box<TableSource>,
        to: Box<TableSource>,
        join_type: i16,
        on_conditions: Vec<String>,
    },
}

impl TableSource {
    fn contains_table(&self, table: &str) -> bool {
        match self {
            TableSource::Simple { name, .. } => name.eq_ignore_ascii_case(table),
            TableSource::Join { from, to, .. } => {
                from.contains_table(table) || to.contains_table(table)
            }
        }
    }

    fn same_join(&mut self, jtype: i16, on: &str) -> bool {
        match self {
            TableSource::Join {
                join_type,
                on_conditions,
                ..
            } => {
                if *join_type == jtype {
                    // AND conditions are added in reverse order
                    on_conditions.insert(0, on.to_string());
                    true
                } else {
                    false
                }
            }
            TableSource::Simple { .. } => false,
        }
    }

    fn to_sql(&self, is_top_level: bool) -> String {
        match self {
            TableSource::Simple { expr, .. } => expr.clone(),
            TableSource::Join {
                from,
                to,
                join_type,
                on_conditions,
            } => {
                let join_str = match join_type {
                    1 => " INNER JOIN ",
                    2 => " LEFT JOIN ",
                    3 => " RIGHT JOIN ",
                    _ => " JOIN ",
                };

                let mut sb = String::new();
                if !is_top_level {
                    sb.push('(');
                }

                sb.push_str(&from.to_sql(false));
                sb.push_str(join_str);
                sb.push_str(&to.to_sql(false));
                sb.push_str(" ON ");

                let multi = on_conditions.len() > 1;
                if multi {
                    sb.push('(');
                }
                sb.push_str(&on_conditions.join(") AND ("));
                if multi {
                    sb.push(')');
                }

                if !is_top_level {
                    sb.push(')');
                }

                sb
            }
        }
    }
}

fn build_from_tables(rows: &[QueryRow]) -> Vec<String> {
    let table_rows = rows_by_attr(rows, ATTR_TABLE);
    let join_rows = rows_by_attr(rows, ATTR_JOIN);

    let mut sources: Vec<TableSource> = Vec::new();
    for trow in &table_rows {
        let mut expr = String::new();
        if let Some(ref e) = trow.expression {
            expr.push_str(&to_quoted_expr(e));
            expr.push('.');
        }
        if let Some(ref n1) = trow.name1 {
            expr.push_str(&to_optional_quoted(n1, true));
        }
        if let Some(ref n2) = trow.name2 {
            expr.push_str(&to_alias(Some(n2)));
        }
        let key = trow
            .name2
            .as_deref()
            .or(trow.name1.as_deref())
            .unwrap_or("")
            .to_string();
        sources.push(TableSource::Simple { name: key, expr });
    }

    for jrow in &join_rows {
        let from_table = match jrow.name1.as_deref() {
            Some(s) => s,
            None => continue,
        };
        let to_table = match jrow.name2.as_deref() {
            Some(s) => s,
            None => continue,
        };
        let on_expr = match jrow.expression.as_deref() {
            Some(s) => s,
            None => continue,
        };
        let jtype = jrow.flag.unwrap_or(1);

        // Find from and to in existing sources
        let mut from_idx = None;
        let mut to_idx = None;
        let mut same_source = false;

        for (i, ts) in sources.iter().enumerate() {
            if from_idx.is_none() && ts.contains_table(from_table) {
                from_idx = Some(i);
                if to_idx.is_none() && ts.contains_table(to_table) {
                    to_idx = from_idx;
                    same_source = true;
                    break;
                }
            } else if to_idx.is_none() && ts.contains_table(to_table) {
                to_idx = Some(i);
            }
            if from_idx.is_some() && to_idx.is_some() {
                break;
            }
        }

        if same_source {
            if let Some(idx) = from_idx {
                if sources[idx].same_join(jtype, on_expr) {
                    continue;
                }
                // Inconsistent join types — skip
                continue;
            }
        }

        // Extract sources, removing from list (higher index first)
        let from_ts;
        let to_ts;

        match (from_idx, to_idx) {
            (Some(fi), Some(ti)) => {
                if fi > ti {
                    from_ts = sources.remove(fi);
                    to_ts = sources.remove(ti);
                } else {
                    to_ts = sources.remove(ti);
                    from_ts = sources.remove(fi);
                }
            }
            (Some(fi), None) => {
                from_ts = sources.remove(fi);
                to_ts = TableSource::Simple {
                    name: to_table.to_string(),
                    expr: to_optional_quoted(to_table, true),
                };
            }
            (None, Some(ti)) => {
                from_ts = TableSource::Simple {
                    name: from_table.to_string(),
                    expr: to_optional_quoted(from_table, true),
                };
                to_ts = sources.remove(ti);
            }
            (None, None) => {
                from_ts = TableSource::Simple {
                    name: from_table.to_string(),
                    expr: to_optional_quoted(from_table, true),
                };
                to_ts = TableSource::Simple {
                    name: to_table.to_string(),
                    expr: to_optional_quoted(to_table, true),
                };
            }
        }

        sources.push(TableSource::Join {
            from: Box::new(from_ts),
            to: Box::new(to_ts),
            join_type: jtype,
            on_conditions: vec![on_expr.to_string()],
        });
    }

    sources.iter().map(|ts| ts.to_sql(true)).collect()
}

// ---------------------------------------------------------------------------
// Core SELECT-like SQL generation
// ---------------------------------------------------------------------------

fn append_select_body(
    builder: &mut String,
    rows: &[QueryRow],
    use_prefix: bool,
    into_target: Option<&str>,
    col_filter: &dyn Fn(&QueryRow) -> bool,
    gb_filter: &dyn Fn(&QueryRow) -> bool,
) {
    if use_prefix {
        builder.push_str("SELECT ");
        let sel_type = get_select_type(rows);
        if !sel_type.is_empty() {
            builder.push_str(&sel_type);
            builder.push(' ');
        }
    }

    builder.push_str(&get_select_columns(rows, col_filter));

    if let Some(target) = into_target {
        builder.push_str(" INTO ");
        builder.push_str(&to_optional_quoted(target, true));
    }

    let from = build_from_tables(rows);
    if !from.is_empty() {
        builder.push_str("\nFROM ");
        builder.push_str(&from.join(", "));
    }

    if let Some(w) = where_expr(rows) {
        builder.push_str("\nWHERE ");
        builder.push_str(w);
    }

    let gb = get_groupings(rows, gb_filter);
    if !gb.is_empty() {
        builder.push_str("\nGROUP BY ");
        builder.push_str(&gb);
    }

    if let Some(h) = having_expr(rows) {
        builder.push_str("\nHAVING ");
        builder.push_str(h);
    }

    let ord = get_orderings(rows);
    if !ord.is_empty() {
        builder.push_str("\nORDER BY ");
        builder.push_str(&ord);
    }
}

// ---------------------------------------------------------------------------
// Query-type-specific SQL
// ---------------------------------------------------------------------------

fn sql_select(builder: &mut String, rows: &[QueryRow]) {
    append_select_body(builder, rows, true, None, &|_| true, &|_| true);
}

fn sql_delete(builder: &mut String, rows: &[QueryRow]) {
    builder.push_str("DELETE ");
    append_select_body(builder, rows, false, None, &|_| true, &|_| true);
}

fn sql_update(builder: &mut String, rows: &[QueryRow]) {
    builder.push_str("UPDATE ");
    let from = build_from_tables(rows);
    builder.push_str(&from.join(", "));

    let column_rows = rows_by_attr(rows, ATTR_COLUMN);
    let set_parts: Vec<String> = column_rows
        .iter()
        .filter_map(|r| {
            let name2 = r.name2.as_deref()?;
            let expr = r.expression.as_deref()?;
            Some(format!(
                "{} = {}",
                to_optional_quoted(name2, true),
                expr
            ))
        })
        .collect();
    if !set_parts.is_empty() {
        builder.push_str("\nSET ");
        builder.push_str(&set_parts.join(", "));
    }

    if let Some(w) = where_expr(rows) {
        builder.push_str("\nWHERE ");
        builder.push_str(w);
    }
}

fn sql_append(builder: &mut String, rows: &[QueryRow]) {
    let target = type_row(rows).and_then(|r| r.name1.as_deref()).unwrap_or("");

    builder.push_str("INSERT INTO ");
    builder.push_str(&to_optional_quoted(target, true));

    // Target columns: column rows with name2 set (that are NOT value rows)
    let all_col_rows = rows_by_attr(rows, ATTR_COLUMN);
    let target_cols: Vec<String> = all_col_rows
        .iter()
        .filter(|r| r.name2.is_some())
        .filter_map(|r| Some(to_optional_quoted(r.name2.as_deref()?, true)))
        .collect();
    if !target_cols.is_empty() {
        builder.push_str(" (");
        builder.push_str(&target_cols.join(", "));
        builder.push(')');
    }

    // Check for VALUES (rows with APPEND_VALUE_FLAG)
    let value_rows: Vec<&&QueryRow> = all_col_rows
        .iter()
        .filter(|r| {
            r.flag
                .map(|f| (f & APPEND_VALUE_FLAG) != 0)
                .unwrap_or(false)
        })
        .collect();

    builder.push('\n');
    if !value_rows.is_empty() {
        let values: Vec<&str> = value_rows
            .iter()
            .filter_map(|r| r.expression.as_deref())
            .collect();
        builder.push_str("VALUES (");
        builder.push_str(&values.join(", "));
        builder.push(')');
    } else {
        // SELECT ... FROM ...
        let not_value = |r: &QueryRow| {
            !r.flag
                .map(|f| (f & APPEND_VALUE_FLAG) != 0)
                .unwrap_or(false)
        };
        append_select_body(builder, rows, true, None, &not_value, &|_| true);
    }
}

fn sql_make_table(builder: &mut String, rows: &[QueryRow]) {
    let target = type_row(rows).and_then(|r| r.name1.as_deref()).unwrap_or("");
    append_select_body(
        builder,
        rows,
        true,
        Some(target),
        &|_| true,
        &|_| true,
    );
}

fn sql_crosstab(builder: &mut String, rows: &[QueryRow]) {
    // TRANSFORM expression: column rows without PIVOT or NORMAL flag
    let all_col_rows = rows_by_attr(rows, ATTR_COLUMN);
    let transform_row = all_col_rows.iter().find(|r| {
        let f = r.flag.unwrap_or(0);
        (f & (CROSSTAB_PIVOT_FLAG | CROSSTAB_NORMAL_FLAG)) == 0
    });
    if let Some(trow) = transform_row {
        if let Some(ref expr) = trow.expression {
            builder.push_str("TRANSFORM ");
            builder.push_str(expr);
            builder.push_str(&to_alias(trow.name1.as_deref()));
            builder.push('\n');
        }
    }

    // SELECT body with NORMAL columns and NORMAL groupby
    let normal_col = |r: &QueryRow| {
        r.flag
            .map(|f| (f & CROSSTAB_NORMAL_FLAG) != 0)
            .unwrap_or(false)
    };
    let normal_gb = |r: &QueryRow| {
        r.flag
            .map(|f| (f & CROSSTAB_NORMAL_FLAG) != 0)
            .unwrap_or(false)
    };
    append_select_body(builder, rows, true, None, &normal_col, &normal_gb);

    // PIVOT expression: column row with PIVOT flag
    let pivot_row = all_col_rows
        .iter()
        .find(|r| {
            r.flag
                .map(|f| (f & CROSSTAB_PIVOT_FLAG) != 0)
                .unwrap_or(false)
        });
    if let Some(prow) = pivot_row {
        if let Some(ref expr) = prow.expression {
            builder.push_str("\nPIVOT ");
            builder.push_str(expr);
        }
    }
}

fn sql_union(builder: &mut String, rows: &[QueryRow]) {
    let table_rows = rows_by_attr(rows, ATTR_TABLE);

    let part1 = table_rows
        .iter()
        .find(|r| r.name2.as_deref() == Some(UNION_PART1))
        .and_then(|r| r.expression.as_deref());
    let part2 = table_rows
        .iter()
        .find(|r| r.name2.as_deref() == Some(UNION_PART2))
        .and_then(|r| r.expression.as_deref());

    if let Some(p1) = part1 {
        let cleaned = clean_union_string(p1);
        builder.push_str(&cleaned);
    }

    builder.push_str("\nUNION ");

    // UNION_FLAG set means regular UNION; unset means UNION ALL
    if !has_flag(rows, UNION_FLAG) {
        builder.push_str("ALL ");
    }

    if let Some(p2) = part2 {
        let cleaned = clean_union_string(p2);
        builder.push_str(&cleaned);
    }

    let ord = get_orderings(rows);
    if !ord.is_empty() {
        builder.push_str("\nORDER BY ");
        builder.push_str(&ord);
    }
}

fn clean_union_string(s: &str) -> String {
    let trimmed = s.trim();
    let mut result = String::with_capacity(trimmed.len());
    let mut prev_newline = false;
    for c in trimmed.chars() {
        if c == '\r' || c == '\n' {
            if !prev_newline {
                result.push('\n');
                prev_newline = true;
            }
            // 連続する改行は無視（圧縮）
        } else {
            result.push(c);
            prev_newline = false;
        }
    }
    result
}

fn sql_passthrough(builder: &mut String, rows: &[QueryRow]) {
    if let Some(expr) = type_row(rows).and_then(|r| r.expression.as_deref()) {
        builder.push_str(expr);
    }
}

fn sql_ddl(builder: &mut String, rows: &[QueryRow]) {
    if let Some(expr) = type_row(rows).and_then(|r| r.expression.as_deref()) {
        builder.push_str(expr);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Unit tests --

    #[test]
    fn query_type_from_flag() {
        assert_eq!(QueryType::from_flag(1), Some(QueryType::Select));
        assert_eq!(QueryType::from_flag(2), Some(QueryType::MakeTable));
        assert_eq!(QueryType::from_flag(3), Some(QueryType::Append));
        assert_eq!(QueryType::from_flag(4), Some(QueryType::Update));
        assert_eq!(QueryType::from_flag(5), Some(QueryType::Delete));
        assert_eq!(QueryType::from_flag(6), Some(QueryType::Crosstab));
        assert_eq!(QueryType::from_flag(7), Some(QueryType::Ddl));
        assert_eq!(QueryType::from_flag(8), Some(QueryType::Passthrough));
        assert_eq!(QueryType::from_flag(9), Some(QueryType::Union));
        assert_eq!(QueryType::from_flag(0), None);
        assert_eq!(QueryType::from_flag(99), None);
    }

    #[test]
    fn join_type_str() {
        // INNER JOIN
        let ts = TableSource::Join {
            from: Box::new(TableSource::Simple {
                name: "T1".to_string(),
                expr: "T1".to_string(),
            }),
            to: Box::new(TableSource::Simple {
                name: "T2".to_string(),
                expr: "T2".to_string(),
            }),
            join_type: 1,
            on_conditions: vec!["T1.id = T2.id".to_string()],
        };
        assert_eq!(ts.to_sql(true), "T1 INNER JOIN T2 ON T1.id = T2.id");

        // LEFT JOIN
        let ts = TableSource::Join {
            from: Box::new(TableSource::Simple {
                name: "T1".to_string(),
                expr: "T1".to_string(),
            }),
            to: Box::new(TableSource::Simple {
                name: "T2".to_string(),
                expr: "T2".to_string(),
            }),
            join_type: 2,
            on_conditions: vec!["T1.id = T2.id".to_string()],
        };
        assert_eq!(ts.to_sql(true), "T1 LEFT JOIN T2 ON T1.id = T2.id");

        // RIGHT JOIN
        let ts = TableSource::Join {
            from: Box::new(TableSource::Simple {
                name: "T1".to_string(),
                expr: "T1".to_string(),
            }),
            to: Box::new(TableSource::Simple {
                name: "T2".to_string(),
                expr: "T2".to_string(),
            }),
            join_type: 3,
            on_conditions: vec!["T1.id = T2.id".to_string()],
        };
        assert_eq!(ts.to_sql(true), "T1 RIGHT JOIN T2 ON T1.id = T2.id");
    }

    #[test]
    fn join_multi_on() {
        let ts = TableSource::Join {
            from: Box::new(TableSource::Simple {
                name: "T1".to_string(),
                expr: "T1".to_string(),
            }),
            to: Box::new(TableSource::Simple {
                name: "T2".to_string(),
                expr: "T2".to_string(),
            }),
            join_type: 1,
            on_conditions: vec![
                "T1.a = T2.a".to_string(),
                "T1.b = T2.b".to_string(),
            ],
        };
        assert_eq!(
            ts.to_sql(true),
            "T1 INNER JOIN T2 ON (T1.a = T2.a) AND (T1.b = T2.b)"
        );
    }

    #[test]
    fn join_nested_parens() {
        let inner = TableSource::Join {
            from: Box::new(TableSource::Simple {
                name: "T1".to_string(),
                expr: "T1".to_string(),
            }),
            to: Box::new(TableSource::Simple {
                name: "T2".to_string(),
                expr: "T2".to_string(),
            }),
            join_type: 1,
            on_conditions: vec!["T1.id = T2.id".to_string()],
        };
        let outer = TableSource::Join {
            from: Box::new(inner),
            to: Box::new(TableSource::Simple {
                name: "T3".to_string(),
                expr: "T3".to_string(),
            }),
            join_type: 2,
            on_conditions: vec!["T1.id = T3.id".to_string()],
        };
        assert_eq!(
            outer.to_sql(true),
            "(T1 INNER JOIN T2 ON T1.id = T2.id) LEFT JOIN T3 ON T1.id = T3.id"
        );
    }

    #[test]
    fn quoting_simple() {
        assert_eq!(to_optional_quoted("Table1", true), "Table1");
        assert_eq!(to_optional_quoted("col1", true), "col1");
    }

    #[test]
    fn quoting_with_space() {
        assert_eq!(to_optional_quoted("Another Table", false), "[Another Table]");
    }

    #[test]
    fn quoting_dotted_identifier() {
        assert_eq!(to_optional_quoted("Table1.col1", true), "Table1.col1");
    }

    #[test]
    fn quoting_already_quoted() {
        assert_eq!(to_optional_quoted("[Table1]", true), "[Table1]");
    }

    // -- Integration tests with real .mdb files --

    fn test_data_path(relative: &str) -> Option<std::path::PathBuf> {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let path = std::path::PathBuf::from(manifest_dir)
            .join("../../testdata")
            .join(relative);
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    macro_rules! skip_if_missing {
        ($path:expr) => {
            match test_data_path($path) {
                Some(p) => p,
                None => {
                    eprintln!("SKIP: test data not found: {}", $path);
                    return;
                }
            }
        };
    }

    #[test]
    fn read_queries_count() {
        let path = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        assert_eq!(queries.len(), 9, "should have 9 queries");
    }

    #[test]
    fn read_queries_names() {
        let path = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        let names: Vec<&str> = queries.iter().map(|q| q.name.as_str()).collect();
        assert!(names.contains(&"SelectQuery"));
        assert!(names.contains(&"DeleteQuery"));
        assert!(names.contains(&"UpdateQuery"));
        assert!(names.contains(&"AppendQuery"));
        assert!(names.contains(&"MakeTableQuery"));
        assert!(names.contains(&"CrosstabQuery"));
        assert!(names.contains(&"UnionQuery"));
        assert!(names.contains(&"PassthroughQuery"));
        assert!(names.contains(&"DataDefinitionQuery"));
    }

    #[test]
    fn read_queries_types() {
        let path = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        for q in &queries {
            let expected = match q.name.as_str() {
                "SelectQuery" => QueryType::Select,
                "DeleteQuery" => QueryType::Delete,
                "UpdateQuery" => QueryType::Update,
                "AppendQuery" => QueryType::Append,
                "MakeTableQuery" => QueryType::MakeTable,
                "CrosstabQuery" => QueryType::Crosstab,
                "UnionQuery" => QueryType::Union,
                "PassthroughQuery" => QueryType::Passthrough,
                "DataDefinitionQuery" => QueryType::Ddl,
                other => panic!("unexpected query name: {other}"),
            };
            assert_eq!(q.query_type, expected, "type mismatch for {}", q.name);
        }
    }

    fn find_query<'a>(queries: &'a [QueryDef], name: &str) -> &'a QueryDef {
        queries.iter().find(|q| q.name == name).unwrap()
    }

    #[test]
    fn sql_delete_query() {
        let path = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        let q = find_query(&queries, "DeleteQuery");
        let sql = query_to_sql(q);
        // Expected:
        // DELETE Table1.col1, Table1.col2, Table1.col3
        // FROM Table1
        // WHERE (((Table1.col1)>"blah"));
        assert!(sql.starts_with("DELETE "), "should start with DELETE: {sql}");
        assert!(sql.contains("Table1.col1"), "should contain Table1.col1: {sql}");
        assert!(sql.contains("FROM Table1"), "should contain FROM Table1: {sql}");
        assert!(
            sql.contains("WHERE (((Table1.col1)>\"blah\"))"),
            "should contain WHERE clause: {sql}"
        );
        assert!(sql.ends_with(';'), "should end with semicolon: {sql}");
    }

    #[test]
    fn sql_select_query() {
        let path = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        let q = find_query(&queries, "SelectQuery");
        let sql = query_to_sql(q);
        assert!(sql.starts_with("SELECT DISTINCT "), "should start with SELECT DISTINCT: {sql}");
        assert!(sql.contains("Table1.*"), "should contain Table1.*: {sql}");
        assert!(sql.contains("Table2.col1"), "should contain Table2.col1: {sql}");
        assert!(
            sql.contains("LEFT JOIN Table3 ON Table1.col1 = Table3.col1"),
            "should contain LEFT JOIN: {sql}"
        );
        assert!(
            sql.contains("INNER JOIN Table2"),
            "should contain INNER JOIN: {sql}"
        );
        assert!(sql.contains("ORDER BY Table2.col1"), "should contain ORDER BY: {sql}");
    }

    #[test]
    fn sql_append_query() {
        let path = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        let q = find_query(&queries, "AppendQuery");
        let sql = query_to_sql(q);
        assert!(
            sql.starts_with("INSERT INTO Table3"),
            "should start with INSERT INTO Table3: {sql}"
        );
        assert!(sql.contains("(col2, col2, col3)"), "should contain target columns: {sql}");
        assert!(sql.contains("SELECT "), "should contain SELECT: {sql}");
        assert!(
            sql.contains("INNER JOIN Table2 ON [Table1].[col1]=[Table2].[col1]"),
            "should contain JOIN: {sql}"
        );
    }

    #[test]
    fn sql_update_query() {
        let path = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        let q = find_query(&queries, "UpdateQuery");
        let sql = query_to_sql(q);
        assert!(
            sql.contains("PARAMETERS User Name Text;"),
            "should contain PARAMETERS: {sql}"
        );
        assert!(sql.contains("UPDATE Table1"), "should contain UPDATE Table1: {sql}");
        assert!(sql.contains("SET "), "should contain SET: {sql}");
        assert!(
            sql.contains("Table1.col1 = \"foo\""),
            "should contain col1 assignment: {sql}"
        );
    }

    #[test]
    fn sql_make_table_query() {
        let path = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        let q = find_query(&queries, "MakeTableQuery");
        let sql = query_to_sql(q);
        assert!(
            sql.contains("INTO Table4"),
            "should contain INTO Table4: {sql}"
        );
        assert!(sql.contains("SELECT "), "should contain SELECT: {sql}");
        assert!(
            sql.contains("Max(Table2.col1) AS MaxOfcol1"),
            "should contain aggregate: {sql}"
        );
        assert!(
            sql.contains("GROUP BY Table2.col2, Table3.col2"),
            "should contain GROUP BY: {sql}"
        );
    }

    #[test]
    fn sql_crosstab_query() {
        let path = skip_if_missing!("V2003/queryTestV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        let q = find_query(&queries, "CrosstabQuery");
        let sql = query_to_sql(q);
        assert!(
            sql.starts_with("TRANSFORM "),
            "should start with TRANSFORM: {sql}"
        );
        assert!(
            sql.contains("Count([Table2].[col2]) AS CountOfcol2"),
            "should contain TRANSFORM expression: {sql}"
        );
        assert!(
            sql.contains("PIVOT [Table1].[col1]"),
            "should contain PIVOT: {sql}"
        );
    }

    #[test]
    fn no_queries_returns_empty() {
        let path = skip_if_missing!("V2003/testV2003.mdb");
        let mut reader = PageReader::open(&path).unwrap();
        let queries = read_queries(&mut reader).unwrap();
        assert!(queries.is_empty(), "testV2003.mdb should have no queries");
    }
}
