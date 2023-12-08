## Rust Slim - Slim server for Rust

Develop Slim Fixtures for rust applications. Based on the [Slim Protocol](https://fitnesse.org/FitNesse/UserGuide/WritingAcceptanceTests/SliM/SlimProtocol.html) of fitnesse [http://fitnesse.org/FrontPage](https://fitnesse.org) .

This is not 100% compliant with the slim protocol right now. Here are some of the features that are known to not be implemented:

It can only handle string symbols and it cannot.
It does not support a SUT for features.
It does not support Actors.
It does not support using the STOUD and STDIN for comunication.

This is currently in an unstable version. The general API can change in the next versions.

For more details, take a look at the [documentation](https://docs.rs/rust_slim/0.1.0/rust_slim/)