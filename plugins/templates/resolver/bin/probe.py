#!/usr/bin/env python3
"""Reads a web page and prints the files it links to, as JSON, for FetchBox's Add download dialog.

Called as: probe.py <page address> <comma-separated extensions>

The shape of the output is whatever `result` in plugin.json reads; here it is
{"entries": [{"name": ..., "url": ..., "size": ...}]}. A non-zero exit code or text on standard
error is shown to the user as the reason the address could not be read.

Python 3 only, no packages. The official Docker image has no interpreter: there, ship a static
program or make it a dependency (see plugins/README.md).
"""
import json
import sys
import urllib.parse
import urllib.request
from html.parser import HTMLParser

MAX_PAGE = 2 * 1024 * 1024
MAX_ENTRIES = 500


class Links(HTMLParser):
    def __init__(self):
        super().__init__()
        self.hrefs = []

    def handle_starttag(self, tag, attrs):
        if tag == "a":
            href = dict(attrs).get("href")
            if href:
                self.hrefs.append(href)


def main():
    if len(sys.argv) < 3:
        sys.exit("usage: probe.py <url> <extensions>")
    page, wanted = sys.argv[1], sys.argv[2]
    extensions = {e.strip().lower().lstrip(".") for e in wanted.split(",") if e.strip()}

    request = urllib.request.Request(page, headers={"User-Agent": "FetchBox-extension"})
    with urllib.request.urlopen(request, timeout=20) as response:
        html = response.read(MAX_PAGE).decode("utf-8", errors="replace")

    parser = Links()
    parser.feed(html)

    entries, seen = [], set()
    for href in parser.hrefs:
        url = urllib.parse.urljoin(page, href)
        parts = urllib.parse.urlsplit(url)
        if parts.scheme not in ("http", "https") or url in seen:
            continue
        name = urllib.parse.unquote(parts.path.rsplit("/", 1)[-1])
        if "." not in name or name.rsplit(".", 1)[-1].lower() not in extensions:
            continue
        seen.add(url)
        entries.append({"name": name, "url": url})
        if len(entries) >= MAX_ENTRIES:
            break

    print(json.dumps({"entries": entries}))


if __name__ == "__main__":
    main()
