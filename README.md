```
d8b                         d8b                 
 ?88                         ?88                 
  88b                         88b                
  888888b  d8888b   88bd88b   888  d88' d888b8b  
  88P `?8bd8P' ?88  88P' ?8b  888bd8P' d8P' ?88  
 d88,  d8888b  d88 d88   88P d88888b   88b  ,88b 
d888888P'`?8888P'd88'   88bd88' `?88b,`?88P'`88b
```

[![GitHub](https://img.shields.io/badge/github-bonka-8da0cb?logo=GitHub)](https://github.com/SeedyROM/bonka)
[![Tests](https://github.com/SeedyROM/bonka/actions/workflows/rust.yml/badge.svg)](https://github.com/SeedyROM/bonka/actions)
[![Codecov](https://codecov.io/gh/SeedyROM/bonka/graph/badge.svg?token=jCYRVFKoQQ)](https://codecov.io/gh/SeedyROM/bonka)
[![License: MIT](https://img.shields.io/badge/license-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![License](https://img.shields.io/badge/license-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

## What does it mean?

**Bonka means nothing, I literally made up a stupid word.**

*However here's a fun backcronym:*

#### BINARY ON NETWORK KEY-VALUE ACCESS

## What is it?

Bonka is a simple key-value store that compiles to a 1.8MB binary *(in release mode)*. It's designed to be used in a networked environment where you need to store some data in a key-value format. This is helpful for systems with incredibly limited resources. An eventual goal is not have any OS dependencies and run on bare metal. For now it's meant to be a simple, fast, and lightweight key-value store.


## Who can use it?

The plan is to create libraries for various languages so that you can use Bonka in your projects.

### Planned Libraries

- [x] [Python](clients/python)
- [ ] Rust
    - This comes directly from the server code since the APIs are the same.
    - The client lib has not been written yet and the current test suite is only from the server.
- [ ] NodeJS


### Possible Libraries

**Long Term**
- [ ] C
- [ ] C++

**Maybe One Day**
- [ ] Ruby Style Langs
    - [ ] Ruby
    - [ ] Crystal
    - [ ] Elixir

## How do I use it?

Clone the repo, and run `cargo run -- run` to start the server. You can then use any client to interact with the server.

**See `cargo run -- --help` for more options and subcommands.**

*Or you can compile the binary with `cargo build --release` (or whatever target you like) and omit `cargo run --` from the above commands if you specify the path of the executable.*

## What's the status?

This is a work in progress. The server is functional, but there are no clients yet. The server is also not optimized at all. It's a simple implementation to get the idea across. There are simple benchmarks setup to track performance improvements using [criterion](https://docs.rs/criterion/latest/criterion/) and (eventually) [flame](https://docs.rs/flame/latest/flame/) tracing.

## Run The Benchmarks

Benchmarking is quite simple. Run `cargo bench`. This will give you a good idea of how the server/underlying APIs is performing and uses [critireon](https://docs.rs/criterion/latest/criterion/) to measure performance and deltas between changes.

## License

This project is licensed under the MIT or Apache 2.0 license, pick which ever works for you! See the [LICENSE](LICENSE) file for more details.

## Contributing

I'm open to contributions, but please open an issue first so we can discuss the changes you'd like to make. I'm also open to suggestions and feedback. This is a pet project/expirement and I'm always looking to learn new things.