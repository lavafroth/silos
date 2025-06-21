# Silos

Dumb, proomptable modular snippet search.

## Getting started

### Installation

Prerequisites:

- libc
- [rust toolchain](https://rustup.rs)

Clone this repository and enter it

``` sh
git clone https://github.com/lavafroth/silos
cd silos
```

### Setup

Add your code snippets as KDL files in the `./snippets/v1/LANGUAGE/` directory, Take a look at the example snippet for golang in `./snippets/v1/go/simple_worker.kdl`.

The snippets must conform to the following structure:

``` kdl
desc "a well articulated description of the snippet",
body #"fn main() { println!("The body of the snippet") }"#
```

KDL supports arbitrary raw strings with as many `#`s before and after the quotes to disambiguate them from the string contents.

After adding your snippets, run the server

``` sh
cargo r
```

> [!NOTE]
>
> Embedding defaults to using the CPU. You may use the `--gpu` flag with a GPU number to use a dedicated GPU.

### Usage

An HTTP REST API listens on port 8000 and can be queried for code snippets.

#### Query a snippet

``` sh
curl http://localhost:8000/api/v1/get --json '{ "desc": "channeled worker in go" }'
```

You must add the "in someLanguage" suffix to your query's description field. This is to keep the API design simple for bothIDE and non-IDE users.

#### Add a snippet

``` sh
curl http://localhost:8000/api/v1/add --json \
'{ "desc": "Build an asynchronous shared mutable state", "lang": "rust", "body": "let object = Arc::new(Mutex::new(old));" }'
```

## v2 API

Language grammar parsing with abstract syntax tree manipulation support.

Coming soon

## TODOs

- [ ] Create an LSP to add the suffix based on filetype.
