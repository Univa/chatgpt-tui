# ChatGPT TUI

Basic terminal user interface for ChatGPT.

https://user-images.githubusercontent.com/41708691/225447685-251ad970-743d-4b5d-beaf-c9c29b812091.mp4

## Installation

Before building, you may need the development packages/headers for OpenSSL (needed by [reqwest](https://github.com/seanmonstar/reqwest#requirements))
and ncurses (needed by [cursive](https://github.com/gyscos/cursive/wiki/Install-ncurses#archlinux)).

To install the binary, run (in this folder):

```
cargo install --bin chat --path .
```

Before running the application, you must specify the `OPENAI_API_KEY` environment
variable. You can get an API key by generating one [here](https://platform.openai.com/account/api-keys).

You can then call `chat` to run the application.

## To-do

- [ ] Saving and continuing past conversations
- [ ] More customization (e.g. choosing models, or UI layout)
- [ ] (More) Markdown rendering

## Notes

- Some markdown AST nodes are not being rendered, so if some responses don't
  look correct, that may be why.
- Sometimes ChatGPT may not include the language tag after code fences. This
  can result in a lack of syntax highlighting for some repsonses containing
  code.

