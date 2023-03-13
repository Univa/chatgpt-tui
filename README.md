# ChatGPT TUI

Basic terminal user interface for ChatGPT.

## Installation

Before building, you may need the development packages/headers for OpenSSL (needed by [reqwest](https://github.com/seanmonstar/reqwest#requirements))
and ncurses (needed by [cursive](https://github.com/gyscos/cursive/wiki/Install-ncurses#archlinux)).

To install the binary, run (in this folder):

```
cargo install --path .
```

Before running the application, you must specify the `OPENAI_API_KEY` environment
variable. You can get an API key by generating one [here](https://platform.openai.com/account/api-keys).

You can then call `chat` to run the application.

## To-do

- [ ] Saving and continuing past conversations
- [ ] Syntax highlighting for code fence blocks
