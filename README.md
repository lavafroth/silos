# Silos

Dumb, proomptable modular snippet search.

## Getting started

There are no binary releases yet.

### From source

Prerequisites:

- libc
- [rust toolchain](https://rustup.rs)

Clone this repository and build it.

``` sh
git clone https://github.com/lavafroth/silos
cd silos
cargo build
```

### LSP

v2.0.0 and above will default to `silos` running as an LSP.

Mutations are defined in the same scheme as the HTTP API. Check out those details below. Point your editor or IDE to the resultant binary `./target/debug/silos`.

#### Editor support

- Helix: There's a demo `.helix` directory provided with this project that uses the LSP for `./examples/example.go`.
- Neovim: Please follow [the official guide](https://neovim.io/doc/user/lsp.html).
- VSCode: I dunno it's too complicated, feel free to send me a PR.

#### Usage

- Write a comment above a paragraph of code, consider the example in examples/example.go

``` go
  resumeFilename := "resume.pdf"
  version := 3
  // silos: change the file basename to that of the parent
  whereIsMyResume :=
    filepath.Base(
      documentsDirectory + "CV" + "_v" + strconv.Itoa(version) + "/" + resumeFilename)
```

The comment must follow the format `silos: ...` as shown.

- Select the code to be modified along with the comment above it.
- Trigger code actions. In helix, this is `space`, `a`.
- Select the option called "ask silos."

### HTTP APIs

To run silos as an HTTP API, supply an additional `http` argument.

``` sh
cargo r http
```

> [!NOTE]
>
> Embedding defaults to using the CPU. You may use the `--gpu` flag with a GPU number to use a dedicated GPU.

An HTTP REST API listens on port 8000 and can be queried for code snippets.

### /api/v1

V1 snippets are stored in the KDL format inside per-language directories under `./snippets/v1`. They must conform to the following structure

``` kdl
desc "describes the snippet"
body #"the snippet itself"#
```

KDL supports arbitrary raw strings with as many `#`s before and after the quotes to disambiguate them from the string contents.

See the example snippet `./snippets/v1/go/simple_worker.kdl` in the go programming language.

#### Querying

We recommend the `jo` CLI to easily generate JSON payloads for the API.

``` sh
jo desc="channeled worker in go" \
curl http://localhost:8000/api/v1/get --json @-
```

You must add the "in someLanguage" suffix to your query's description field. This was a bad design choice and will be deprecated in a later release.

#### Adding a snippet

``` sh
curl http://localhost:8000/api/v1/add --json \
'{ "desc": "Build an asynchronous shared mutable state", "lang": "rust", "body": "let object = Arc::new(Mutex::new(old));" }'
```

### /api/v2

This API parses code into an AST (Abstract Syntax Tree) via tree-sitter and can perform subsequent mutations.

#### Supported Languages

- C
- Rust
- Go

#### Defining mutation collections

``` kdl
description "describes the mutation collection"
mutation {
  expression "some ((beautiful) @adjective) AST expression"
  substitute {
    literal "hello"
    capture "adjective"
    literal "world"
  }
}

mutation {
  expression "another"
  substitute {
    literal "multiple mutations work"
    literal "as long as their expression"
    literal "don't collide"
  }
}
```

- `description`: A textual description of the mutation collection.
- `mutation`:  Defines individual code changes.
  - `expression`: Uses tree-sitter to match and capture AST nodes with `@` prefixes, The special `@root` node is reserved for the entire expression.
  - `substitute`:  Constructs the modified code using literals and captured arguments.

See the example mutation collection in `./snippets/v2/go/mutations.kdl`.

#### Querying

``` sh
jo body=@examples/example.go \
desc='change the current filepath to the parent filepath in go' \
| curl http://localhost:8000/api/v2/get --json @-
```

V2 queries have the following fields

- `desc`: Description of the query.
- `body`:  The code to be parsed and modified.

The API performs a single-pass substitution based on the closest matching mutation. Captured groups are used within the `substitute` block and the mutated code is returned in the response JSON `body` field.

**Further reading**

- [tree-sitter query snytax](https://tree-sitter.github.io/tree-sitter/using-parsers/queries/1-syntax.html) to create mutation expressions.
- [jo](https://github.com/jpmens/jo) to build the JSON body from a file.
