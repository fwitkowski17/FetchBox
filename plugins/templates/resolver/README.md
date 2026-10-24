# __NAME__

A FetchBox extension made from the `resolver` template: it teaches **Add download** about the pages you list. Paste
the address of a page, FetchBox runs `bin/probe.py`, and the files the page links to appear as a list to choose from.
The files themselves are downloaded by FetchBox's own engine.

## Try it

1. Put this folder inside `<data dir>/plugins/` and start FetchBox. For the dev backend that is `./.dev/data/plugins/`:
   `cargo run -p server -- --data-dir ./.dev/data --download-dir ./.dev/downloads`.
2. **Extensions → Rescan**, review the permissions, approve, switch it on.
3. **Settings → Extensions**: under *Sites* add a pattern, for example `http://127.0.0.1:8000/*`, and save.
4. Serve a folder with some files: `python3 -m http.server 8000` (it makes a directory page with links).
   Paste `http://127.0.0.1:8000/` into **Add download**.

Test the script on its own first, it needs nothing from FetchBox:

```sh
./bin/probe.py http://127.0.0.1:8000/ jpg,zip
```

Editing `plugin.json` puts the extension back to *needs review*; editing `bin/probe.py` does not.

## Where to go from here

- `result` in `plugin.json` maps the JSON the script prints into the list (JSON Pointers); add `size` when you know it.
- Make the file with your own program instead of the engine: add `run` and `choices` to the resolver, and
  `${dep:...}` dependencies for the program (see `plugins/official/yt-dlp`).
- Python is not in the official Docker image. For that image ship a static binary or a dependency instead of a script.

The full guide is `plugins/README.md` in the FetchBox repository.
