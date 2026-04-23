use std::collections::HashMap;
use std::path::Path;
use tracing::debug;
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

        mutations.push(Mutation {
            expression: expression.to_string(),
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
    let mut split_positions = vec![];
    let mut rewrites = HashMap::new();
    for mutation in &mutations.mutations {
        for query_result in query(root_node, mutation.expression.as_str(), &lang, source_bytes) {
            debug!("mutation query expression matched: {query_result:?}");
            split_positions.push(query_result.start);
            split_positions.push(query_result.end);

            let ast_rewrite: String = mutation
                .substitute
                .iter()
                .map(|substitute| match substitute {
                    Substitute::Literal(literal) => literal.as_str(),
                    Substitute::Capture(attrib) => query_result.captures[attrib].as_str(),
                })
                .collect();
            debug!("AST rewritten to {ast_rewrite:?}");

            rewrites.insert(query_result.start, ast_rewrite);
        }
    }
    split_positions.sort();

    let source_bytes_split = split_positions
        .into_iter()
        .chain(std::iter::once(source_bytes.len()))
        .scan(0, |start, end| {
            let mutation_split = (*start, &source_bytes[*start..end]);
            *start = end;
            Some(mutation_split)
        });

    let mut output = String::default();
    for (i, split) in source_bytes_split {
        let split = std::str::from_utf8(split)?;
        let after_mutation = rewrites.get(&i).map(|v| v.as_str()).unwrap_or(split);
        output.push_str(after_mutation);
    }
    Ok(output)
}

#[derive(Debug)]
pub struct QueryCooked {
    captures: HashMap<String, String>,
    end: usize,
    start: usize,
}

pub fn query<'a>(
    node: Node<'a>,
    expr: &'a str,
    lang: &Language,
    source_bytes: &[u8],
) -> Vec<QueryCooked> {
    let query = Query::new(lang, expr).unwrap();

    let mut qc = QueryCursor::new();
    let mut query_matches = qc.matches(&query, node, source_bytes);

    let capture_names = query.capture_names();
    // println!("names: {capture_names:#?}");

    let mut cooked = vec![];

    while let Some(matcha) = query_matches.next() {
        let mut captures = HashMap::new();
        let mut root = 0..0;
        if matcha.captures.is_empty() {
            continue;
        }
        //     println!("match {:#?}", matcha.id());

        for (ix, name) in capture_names.iter().enumerate() {
            let nodes = matcha.nodes_for_capture_index(ix.try_into().unwrap());
            let mut start_pos = None;
            let mut end_pos = None;
            debug!("matches for {name}");
            for node in nodes {
                start_pos.get_or_insert(node.start_byte());
                end_pos.replace(node.end_byte());
                debug!("hit {node:#?}");
            }

            let (Some(start_pos), Some(end_pos)) = (start_pos, end_pos) else {
                continue;
            };

            if *name == "root" {
                root = start_pos..end_pos;
            }

            let text_bytes = &source_bytes[start_pos..end_pos];
            let text = std::str::from_utf8(text_bytes).unwrap();
            //         println!("text: {text}");
            captures.insert(name.to_string(), text.to_string());
        }
        cooked.push(QueryCooked {
            start: root.start,
            end: root.end,
            captures,
        })
    }
    cooked
}
