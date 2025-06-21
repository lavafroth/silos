use std::collections::HashMap;
use std::path::Path;
use tracing::debug_span;
use tree_sitter::{Language, Node, Query, QueryCursor, StreamingIterator};

use anyhow::{Result, bail};
use kdl::KdlDocument;

#[derive(Debug)]
pub struct Mutation {
    pub expression: String,
    pub substitute: Vec<Substitute>,
}

pub struct MutationCollection {
    pub description: String,
    pub mutations: Vec<Mutation>,
}

#[derive(Debug)]
pub enum Substitute {
    Literal(String),
    Capture(String),
}

pub fn from_path<P: AsRef<Path>>(path: P) -> Result<MutationCollection> {
    let contents = std::fs::read_to_string(path)?;
    let doc: KdlDocument = contents.parse()?;
    let mut mutations = vec![];

    let mut description = None;

    for node in doc.nodes() {
        let node_name = node.name().value();

        if node_name != "mutation" && node_name != "description" {
            bail!(
                "document root must only contain `mutation` or `description` nodes: got {node_name}"
            );
        }

        if node_name == "description" {
            description.replace(
                node.entry(0)
                    .unwrap()
                    .value()
                    .as_string()
                    .unwrap()
                    .to_string(),
            );
            continue;
        }

        let node = node.children().unwrap();
        let Some(expression) = node.get_arg("expression").and_then(|v| v.as_string()) else {
            bail!("mutation node must contain an expression");
        };
        let Some(substitute) = node.get("substitute") else {
            bail!("mutation node must contain an substitute");
        };

        let children = substitute.children().unwrap().nodes();
        let mut substitute = vec![];
        for child in children {
            let attrib = child.entry(0).unwrap().value().as_string().unwrap();
            let substitutor = match child.name().value() {
                "literal" => Substitute::Literal(attrib.to_string()),
                "capture" => Substitute::Capture(attrib.to_string()),
                _ => unreachable!(),
            };

            substitute.push(substitutor);
        }

        let expression = expression.to_string();

        mutations.push(Mutation {
            expression,
            substitute,
        })
    }

    let Some(description) = description else {
        bail!("mutation collection contains no `description`");
    };

    Ok(MutationCollection {
        description,
        mutations,
    })
}

pub fn apply(
    lang: Language,
    source_bytes: &[u8],
    root_node: Node<'_>,
    mutations: &MutationCollection,
) -> Result<String, anyhow::Error> {
    let mut split_ats = vec![];
    let mut query_result_map = HashMap::new();
    for mutation in &mutations.mutations {
        let query_result = query(root_node, mutation.expression.as_str(), &lang, source_bytes);
        debug_span!("mutation query expression matched: {query_result:?}");
        split_ats.push(query_result.start);
        split_ats.push(query_result.end);

        let mut ast_rewrite = String::default();
        for sub in &mutation.substitute {
            ast_rewrite.push_str(match sub {
                Substitute::Literal(attrib) => attrib,
                Substitute::Capture(attrib) => &query_result.captures[attrib],
            })
        }
        debug_span!("AST rewritten to {ast_rewrite:?}");

        query_result_map.insert(query_result.start, ast_rewrite);
    }
    split_ats.sort();
    let splits = split_at_indices(source_bytes, &split_ats);
    let mut output = String::default();
    for (i, split) in splits.indices.iter().zip(splits.values) {
        let split = std::str::from_utf8(split)?;
        output.push_str(query_result_map.get(i).map(|v| v.as_str()).unwrap_or(split));
    }
    Ok(output)
}

#[derive(Debug)]
struct QueryCooked {
    captures: HashMap<String, String>,
    end: usize,
    start: usize,
}

pub struct SplitMap<'a> {
    values: Vec<&'a [u8]>,
    indices: Vec<usize>,
}

fn split_at_indices<'a>(c: &'a [u8], idx: &[usize]) -> SplitMap<'a> {
    let mut a = 0;
    let mut values = vec![];
    let mut indices = vec![a];
    for &b in idx {
        values.push(&c[a..b]);
        a = b;
        indices.push(a);
    }
    values.push(&c[a..]);
    assert_eq!(values.len(), indices.len());
    SplitMap { values, indices }
}

fn query<'a>(node: Node<'a>, expr: &'a str, lang: &Language, source_bytes: &[u8]) -> QueryCooked {
    let query = Query::new(lang, expr).unwrap();

    let mut qc = QueryCursor::new();
    let mut query_matches = qc.matches(&query, node, source_bytes);

    let capture_names = query.capture_names();
    let mut capture_cooked = HashMap::new();

    let mut start = 0;
    let mut end = 0;

    if let Some(matcha) = query_matches.next() {
        for cap in matcha.captures {
            let Some(name) = capture_names.get(cap.index as usize) else {
                continue;
            };
            if *name == "root" {
                start = cap.node.start_byte();
                end = cap.node.end_byte();
                continue;
            }
            capture_cooked.insert(
                name.to_string(),
                cap.node.utf8_text(source_bytes).unwrap().to_string(),
            );
        }
    }

    QueryCooked {
        start,
        end,
        captures: capture_cooked,
    }
}
