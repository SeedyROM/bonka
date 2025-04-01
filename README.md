# 📓 Bonka

## What does it mean?

Bonka means nothing, I literally made up a stupid word. 

However here's a fun backcronym:

#### BINARY ON NETWORK KEY-VALUE ACCESS

## What is it?

Bonka is a simple key-value store that compiles to a 1.8MB binary *(in release mode)*. It's designed to be used in a networked environment where you need to store some data in a key-value format. This is helpful for systems with incredibly limited resources. An eventual goal is not have any OS dependencies and run on bare metal. For now it's meant to be a simple, fast, and lightweight key-value store.


## Who can use it?

The plan is to create libraries for various languages so that you can use Bonka in your projects.

#### Planned Libraries

- [ ] Rust
    - This comes directly from the server code since the APIs are the same.
- [ ] Python
- [ ] NodeJS


#### Possible Libraries

- [ ] C
- [ ] C++
- [ ] Ruby Style Langs
    - [ ] Ruby
    - [ ] Crystal
    - [ ] Elixir

## How do I use it?

Clone the repo, and run `cargo run -- run` to start the server. You can then use any client to interact with the server.

**See `cargo run -- --help` for more options and subcommands.**

*Or you can compile the binary with `cargo build --release` (or whatever target you like) and omit `cargo run --` from the above commands.*

## What's the status?

This is a work in progress. The server is functional, but there are no clients yet. The server is also not optimized at all. It's a simple implementation to get the idea across. There are simple benchmarks setup to track performance improvements using [critireon](https://docs.rs/criterion/latest/criterion/) and (eventually) [flame](https://docs.rs/flame/latest/flame/) tracing.