
# Temoc

Acceptance tests in (GitHub Flavored) markdown files. 

## Why?

The idea is so you are able to write simple documentations with accepatance tests, commit it together with your code and validate it in your CI pipeline. Markdown files are universal, easy to write and understand so they are perfect as a medium to have your acceptance tests.

We are using the GitHub Flavored markdown beacuse we need support for [tables](https://github.github.com/gfm/#tables-extension-).

## Why not [Fitnesse](https://fitnesse.org/)?

This project is heavily inspired on fitnesse. We even use the same Slim Protocol to communicate with the SUT(System Under Test). The idea is not to compete with fitnesse, but simply be an alternative. If you want a more feature rich software, you probably should use Fitnesse.

## How to test my project?

We use the [Slim Protocol](https://fitnesse.org/FitNesse/UserGuide/WritingAcceptanceTests/SliM/SlimProtocol.html) to talk with the system under test. [Here](https://fitnesse.org/PlugIns.html) you can find a list of plugins for it in multiple languages. And [Here](https://github.com/killertux/temoc/tree/master/rust_slim) you can find an incomplete implementation for Rust.

You will use one of these plugins to write the test fixtures in your project. Fixtures are glue code that serves as intermediary between Temoc and your software.

After this is done, you can write your accepatance tests markdown. Take a look at our calculator example [Here](https://github.com/killertux/temoc/tree/master/temoc/examples). See the raw markdown files because there are hidden isntructions.

## Building Temoc

Temoc is written in rust, you can install the rust toolchain [here](https://rustup.rs/). After that, you can compile everything using `cargo build --release`. This should create a compiled binary in `./target/release/temoc`.

## Running Temoc

Once you have compiled it, you can run `./temoc --help` to see the list of commands. Basically you will need to specify a command to start the slim server (your plugin should help you on how to do this), a port to be used in the connection (current we don't support STDIN,STOUT communication) and a list of markdown files to test. You can also write a configuration file to have a default list of parameters, you can look at an example [here](https://github.com/killertux/temoc/tree/master/Config.toml.example)