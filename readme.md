
# Temoc

Acceptance tests in (GitHub Flavored) markdown files. 

## Why?

The idea is so you are able to write simple documentations with accepatance tests, commit it together with your code and validate it in your CI pipeline. Markdown files are universal, easy to write and understand so they are perfect as a medium to have your acceptance tests.

We are using the GitHub Flavored markdown beacuse we need support for [tables](https://github.github.com/gfm/#tables-extension-).

## Why not [Fitnesse](https://fitnesse.org/)?

This project is heavily inspired on fitnesse. We even use the same Slim Protocol to communicate with the SUT(System Under Test). The idea is not to compete with fitnesse, but simply be an alternative. If you want a more feature rich software, you probably should use Fitnesse.

## How to test my project?

We use the [Slim Protocol](https://fitnesse.org/FitNesse/UserGuide/WritingAcceptanceTests/SliM/SlimProtocol.html) to talk with the system under test. [Here](https://fitnesse.org/PlugIns.html) you can find a list of plugins for it in multiple languages. And [Here](https://github.com/killertux/temoc/rust_slim) you can find an incomplete implementation for Rust.

You will use one of these plugins to write the test fixtures in your project. Fixtures are glue code that serves as intermediary between Temoc and your software.

After this is done, you can write your accepatance tests markdown. Take a look at our calculator example [Here](https://github.com/killertux/temoc/temoc/examples). See the raw markdown files because there are hidden isntructions.
