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

Add your code snippets in the `./snippets/v1/LANGUAGE/` directory as JSON files, where LANGUAGE is some programming language.

The snippets must conform to the following structure:

``` json
{
  "desc": "a well articulated description of the snippet",
  "body": "fn main() { println!(\"The body of the snippet\") }"
}
```

After adding your snippets, run the server

> [!NOTE]
>
> You may remove the `--cpu` flag if you wish to use a dedicated GPU for embedding text.

```
cargo r --cpu
```

### Usage

The server spawns an HTTP REST API at localhost:8000 which can be queried to fetch code snippet.

```sh
curl http://localhost:8000/api/v1/get \
-H 'content-type: application/json' \
--data '{ "desc": "channeled worker in go" }'
```

You must add the "in someLanguage" suffix to your query's description field. This is to keep the API design simple for both
IDE and non-IDE users. In the future, we aim to create an IDE extension that adds the suffix automatically based on the detected filetype.
