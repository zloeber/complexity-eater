#!/usr/bin/env python3
"""Build and check the static public website."""

import html
import json
import re
import shutil
import sys
import urllib.error
import urllib.request
from html.parser import HTMLParser
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "website"
DIST = SRC / "dist"
CANONICAL = "https://complexityeater.com"
FRONT = re.compile(r"\A<!--\n(.*?)\n-->\n", re.S)
SECRET_QUERY = re.compile(r"(invite|secret|token|session|authorization)", re.I)
FORBIDDEN = (
    "-----BEGIN ",
    "AKIA",
    "ghp_",
    "github_pat_",
    "xoxb-",
    "CLOUDFLARE_API_TOKEN=",
)

NAV = (
    ("product", "/", "Product"),
    ("start", "/start.html", "Start"),
    ("install", "/install.html", "Install"),
    ("architecture", "/architecture.html", "Architecture"),
    ("security", "/security.html", "Security"),
    ("faq", "/faq.html", "FAQ"),
    ("status", "/status.html", "Status"),
)

FOOTER = (
    ("/privacy.html", "Privacy"),
    ("/hosted.html", "Hosted rooms"),
    ("/status.html", "Status"),
    ("https://zloeber.github.io/complexity-eater/", "Documentation"),
    ("https://github.com/zloeber/complexity-eater", "Source"),
    ("https://github.com/zloeber/complexity-eater/blob/main/LICENSE", "MIT license"),
)


def fail(message):
    raise SystemExit(message)


def esc(value):
    return html.escape(value, quote=True)


def parse_page(path):
    text = path.read_text()
    match = FRONT.match(text)
    if not match:
        fail(f"{path} is missing a front-matter comment")
    meta = {}
    for line in match.group(1).splitlines():
        if ": " not in line:
            fail(f"{path} has a bad front-matter line: {line}")
        key, value = line.split(": ", 1)
        meta[key] = value
    for key in ("title", "description", "path", "nav"):
        if key not in meta:
            fail(f"{path} is missing {key}")
    return meta, text[match.end() :]


def toolchain():
    text = (ROOT / "rust-toolchain.toml").read_text()
    match = re.search(r'(?m)^channel\s*=\s*"([^"]+)"', text)
    if not match:
        fail("rust-toolchain.toml has no channel")
    return match.group(1)


def release_manifest():
    path = SRC / "releases" / "v0.2.1.json"
    payload = json.loads(path.read_text())
    required = (
        "version",
        "tag",
        "release_page",
        "releases_index",
        "checksums",
        "image",
        "image_digest",
        "image_digest_file",
        "archives",
    )
    for key in required:
        if key not in payload:
            fail(f"release manifest missing {key}")
    if payload["tag"] != f"v{payload['version']}":
        fail("release tag does not match the version")
    if len(payload["archives"]) != 4:
        fail("the published release has four archives")
    return payload


def release_rows(payload):
    rows = []
    for archive in payload["archives"]:
        rows.append(
            "<tr>"
            f"<td>{esc(archive['os'])}, {esc(archive['cpu'])}</td>"
            f"<td><a href=\"{esc(archive['url'])}\">{esc(archive['file'])}</a></td>"
            "</tr>"
        )
    return "\n".join(rows)


def apply_tokens(body, payload, channel):
    replacements = {
        "{{release.version}}": esc(payload["version"]),
        "{{release.tag}}": esc(payload["tag"]),
        "{{release.page}}": esc(payload["release_page"]),
        "{{release.releases_index}}": esc(payload["releases_index"]),
        "{{release.checksums}}": esc(payload["checksums"]),
        "{{release.image}}": esc(payload["image"]),
        "{{release.image_digest}}": esc(payload["image_digest"]),
        "{{release.image_digest_file}}": esc(payload["image_digest_file"]),
        "{{release.rows}}": release_rows(payload),
        "{{toolchain}}": esc(channel),
    }
    for token, value in replacements.items():
        body = body.replace(token, value)
    if "{{" in body:
        fail("a page still contains an unreplaced token")
    return body


def layout(meta, body):
    path = meta["path"]
    canonical = CANONICAL + "/" if path == "/" else CANONICAL + path
    nav = []
    for key, href, label in NAV:
        current = ' aria-current="page"' if key == meta["nav"] else ""
        nav.append(f'<a href="{esc(href)}"{current}>{esc(label)}</a>')
    footer = []
    for href, label in FOOTER:
        footer.append(f'<a href="{esc(href)}">{esc(label)}</a>')
    script = meta.get("script", "")
    extra = f'<script src="{esc(script)}"></script>' if script else ""
    description = meta["description"]
    title = meta["title"]
    return f"""<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{esc(title)}</title>
  <meta name="description" content="{esc(description)}">
  <link rel="canonical" href="{esc(canonical)}">
  <meta property="og:title" content="{esc(title)}">
  <meta property="og:description" content="{esc(description)}">
  <meta property="og:type" content="website">
  <meta property="og:url" content="{esc(canonical)}">
  <meta property="og:image" content="{CANONICAL}/assets/ce-banner.png">
  <link rel="icon" href="/assets/favicon.svg" type="image/svg+xml">
  <link rel="stylesheet" href="/assets/site.css">
  <script src="/assets/theme-init.js"></script>
</head>
<body>
  <a class="skip" href="#content">Skip to content</a>
  <header class="site-header">
    <a class="brand" href="/">Complexity Eater</a>
    <button class="nav-toggle" type="button" aria-expanded="false" aria-controls="site-nav">Menu</button>
    <nav id="site-nav" class="site-nav" aria-label="Primary">
      {''.join(nav)}
    </nav>
    <button class="theme-toggle" id="theme-toggle" type="button" aria-pressed="false">Use dark theme</button>
  </header>
  {body}
  <footer class="site-footer">
    <p>Different minds. Shared work. The website does not provision a room.</p>
    <nav aria-label="Footer">
      {''.join(footer)}
    </nav>
  </footer>
  <script src="/assets/site.js"></script>
  {extra}
</body>
</html>
"""


def build():
    if DIST.exists():
        shutil.rmtree(DIST)
    DIST.mkdir(parents=True)
    (DIST / "assets").mkdir()
    (DIST / "contracts").mkdir()
    payload = release_manifest()
    channel = toolchain()
    pages = []
    for path in sorted((SRC / "pages").glob("*.html")):
        meta, body = parse_page(path)
        body = apply_tokens(body, payload, channel)
        document = layout(meta, body)
        name = "index.html" if meta["path"] == "/" else meta["path"].lstrip("/")
        target = DIST / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(document)
        pages.append(meta["path"])
    for asset in (SRC / "assets").iterdir():
        if asset.is_file():
            (DIST / "assets" / asset.name).write_bytes(asset.read_bytes())
    banner = ROOT / "docs" / "assets" / "ce-banner.png"
    if not banner.is_file():
        fail("docs/assets/ce-banner.png is missing")
    (DIST / "assets" / "ce-banner.png").write_bytes(banner.read_bytes())
    (DIST / "contracts" / "room-launch.v1.json").write_text(
        (SRC / "contracts" / "room-launch.v1.json").read_text()
    )
    (DIST / "robots.txt").write_text((SRC / "robots.txt").read_text())
    (DIST / "_headers").write_text((SRC / "_headers").read_text())
    locs = []
    for page in pages:
        url = CANONICAL + "/" if page == "/" else CANONICAL + page
        locs.append(f"  <url><loc>{esc(url)}</loc></url>")
    sitemap = (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n'
        + "\n".join(locs)
        + "\n</urlset>\n"
    )
    (DIST / "sitemap.xml").write_text(sitemap)
    return pages


class Collector(HTMLParser):
    def __init__(self):
        super().__init__()
        self.ids = []
        self.hrefs = []
        self.srcs = []
        self.imgs = []
        self.buttons = []
        self.h1 = 0
        self.skip = False
        self.title = False
        self.canonical = ""
        self.description = ""
        self.scripts = []
        self.lang = ""

    def handle_starttag(self, tag, attrs):
        attributes = dict(attrs)
        if tag == "html":
            self.lang = attributes.get("lang", "")
        if "id" in attributes:
            self.ids.append(attributes["id"])
        if tag == "a":
            href = attributes.get("href", "")
            self.hrefs.append(href)
            if href == "#content":
                self.skip = True
        if tag == "img":
            self.imgs.append(attributes.get("alt"))
            self.srcs.append(attributes.get("src", ""))
        if tag in {"script", "link"} and attributes.get("src"):
            self.srcs.append(attributes["src"])
        if tag == "script" and attributes.get("src"):
            self.scripts.append(attributes["src"])
        if tag == "link" and attributes.get("href"):
            self.srcs.append(attributes["href"])
            if attributes.get("rel") == "canonical":
                self.canonical = attributes["href"]
        if tag == "meta" and attributes.get("name") == "description":
            self.description = attributes.get("content", "")
        if tag == "title":
            self.title = True
        if tag == "h1":
            self.h1 += 1
        if tag == "button":
            self.buttons.append(attributes.get("type"))


def resolve_local(page_path, reference):
    if reference.startswith("/"):
        relative = reference.lstrip("/") or "index.html"
        return DIST / relative
    return (page_path.parent / reference).resolve()


def check_document(path, parser):
    if parser.lang != "en":
        fail(f"{path} is missing lang=en")
    if parser.h1 != 1:
        fail(f"{path} has {parser.h1} h1 elements")
    if "content" not in parser.ids:
        fail(f"{path} is missing main#content")
    if not parser.skip:
        fail(f"{path} is missing the skip link")
    if not parser.canonical.startswith(CANONICAL):
        fail(f"{path} has a bad canonical URL")
    if not parser.description:
        fail(f"{path} is missing a description")
    for alt in parser.imgs:
        if not alt:
            fail(f"{path} has an image without alt text")
    for button_type in parser.buttons:
        if button_type != "button":
            fail(f"{path} has a button without type=button")
    for source in parser.scripts:
        if source.startswith("http:") or source.startswith("https:") or source.startswith("//"):
            fail(f"{path} loads a remote script: {source}")
    for reference in parser.hrefs + parser.srcs:
        if not reference or reference.startswith("#") or reference.startswith("mailto:"):
            continue
        if SECRET_QUERY.search(reference) and "?" in reference:
            fail(f"{path} puts a credential-like query on {reference}")
        if reference.startswith("http://"):
            fail(f"{path} uses an insecure URL: {reference}")
        if reference.startswith("https://") or reference.startswith("data:"):
            continue
        target = resolve_local(path, reference.split("#", 1)[0].split("?", 1)[0])
        if not target.is_file():
            fail(f"{path} links to missing {reference}")


def external_urls(dist_root):
    found = []
    for path in dist_root.rglob("*.html"):
        parser = Collector()
        parser.feed(path.read_text())
        for reference in parser.hrefs:
            if reference.startswith("https://"):
                found.append(reference.split("#", 1)[0])
    return sorted(set(found))


def head(url):
    request = urllib.request.Request(
        url,
        method="GET",
        headers={"User-Agent": "complexity-eater-website-check"},
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            status = response.status
            response.read(64)
    except urllib.error.HTTPError as error:
        status = error.code
    if status >= 400:
        fail(f"{url} returned {status}")


def check_contract():
    payload = json.loads((SRC / "contracts" / "room-launch.v1.json").read_text())
    if payload.get("contract") != "commonroom.room-launch":
        fail("room-launch contract name changed")
    if payload.get("version") != "1":
        fail("room-launch contract version changed")
    if payload.get("transmits_workspace") is not False or payload.get("provisions_room") is not False:
        fail("the website contract must not transmit a workspace or provision a room")
    hosted = payload["modes"]["hosted"]
    if hosted.get("status") != "not_implemented" or hosted.get("provisions_room") is not False:
        fail("hosted mode must stay not_implemented")
    if hosted.get("provider_endpoint") is not None:
        fail("contract version 1 has no provider endpoint")
    local = payload["modes"]["local"]
    if local.get("invitation_prefix") != "commonroom://join/":
        fail("invitation prefix changed")
    if local["host"].get("mcp_revision") != "2026-07-28":
        fail("MCP revision changed")
    cli = (ROOT / "crates" / "ce-cli" / "src" / "cli.rs").read_text()
    start = cli.find("const COMMANDS: &[&str] = &[")
    end = cli.find("];", start)
    if start < 0 or end < 0:
        fail("could not read the ce command list")
    commands = re.findall(r'"([^"]+)"', cli[start:end])
    templates = [
        *local["host"]["equivalent"],
        local["transports"]["loopback"]["command"],
        local["transports"]["tailcat"]["command"],
        local["invite_command"],
        local["join_command"],
        local["mcp_command"],
    ]
    flags = {"--name", "--tailcat", "--workspace", "--json", "--no-hold", "--invite", "--ephemeral"}
    for template in templates:
        if any(char in template for char in "\n;&|$`"):
            fail(f"command template is not a single command: {template}")
        tokens = template.split()
        if tokens[0] != "ce":
            fail(f"command does not start with ce: {template}")
        second = tokens[1]
        if second[0] not in "{'<" and second not in commands:
            fail(f"{second} is not a ce command")
        for token in tokens:
            if token.startswith("--") and token not in flags:
                fail(f"unsupported flag in room-launch contract: {token}")
    if local["agent_paste"]["command"] != "ce" or local["agent_paste"]["args"][0] != "mcp":
        fail("agent paste block does not match ce mcp")
    if "default_value(\"read,observe,propose,checkout,commit\")" not in cli:
        fail("default invitation capabilities changed in the CLI")
    expected = ["read", "observe", "propose", "checkout", "commit"]
    if local["default_capabilities"] != expected:
        fail("contract capabilities do not match the CLI default")


def check(online):
    pages = list(DIST.glob("*.html"))
    if len(pages) < 8:
        fail("the website build did not produce the expected pages")
    for path in pages:
        text = path.read_text()
        if not text.startswith("<!DOCTYPE html>"):
            fail(f"{path} is missing a doctype")
        parser = Collector()
        parser.feed(text)
        check_document(path, parser)
        for marker in FORBIDDEN:
            if marker in text:
                fail(f"{path.name} contains a forbidden marker")
    contract_copy = DIST / "contracts" / "room-launch.v1.json"
    if not contract_copy.is_file():
        fail("the launch contract was not published")
    banner = DIST / "assets" / "ce-banner.png"
    if not banner.is_file() or banner.stat().st_size < 1000:
        fail("the banner asset was not copied into the site")
    index = (DIST / "index.html").read_text()
    if "Different minds. Shared work." not in index:
        fail("the tagline is missing from the home page")
    start = (DIST / "start.html").read_text()
    if "does not provision a hosted room" not in start:
        fail("the start page must say that no hosted room is provisioned")
    if 'name="invite"' in start or "type=\"file\"" in start:
        fail("the start page must not collect an invitation or a file upload")
    install = (DIST / "install.html").read_text()
    payload = release_manifest()
    for archive in payload["archives"]:
        if archive["url"] not in install:
            fail(f"install page is missing {archive['file']}")
    if payload["image_digest"] not in install:
        fail("install page is missing the published image digest")
    if "Homebrew" not in install or "Windows" not in install:
        fail("install page must say which platforms are unpublished")
    check_contract()
    sitemap = (DIST / "sitemap.xml").read_text()
    for path in pages:
        leaf = "/" if path.name == "index.html" else f"/{path.name}"
        if CANONICAL + ("" if leaf == "/" else leaf) not in sitemap and not (
            leaf == "/" and f"{CANONICAL}/" in sitemap
        ):
            fail(f"sitemap is missing {leaf}")
    if online:
        for url in external_urls(DIST):
            head(url)
        digest = urllib.request.urlopen(
            urllib.request.Request(
                payload["image_digest_file"],
                headers={"User-Agent": "complexity-eater-website-check"},
            ),
            timeout=30,
        ).read().decode()
        expected = f"{payload['image_digest']}  {payload['image']}"
        if expected not in digest:
            fail("image digest does not match the published release file")


def main():
    if len(sys.argv) < 2 or sys.argv[1] not in {"build", "check"}:
        fail("usage: website.py build|check [--offline]")
    if sys.argv[1] == "build":
        build()
        return
    build()
    check("--offline" not in sys.argv[2:])


if __name__ == "__main__":
    main()
