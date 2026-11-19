"""Check the crawler-visible cards on a served Pattern Room export."""
import hashlib
import struct
import sys
from concurrent.futures import ThreadPoolExecutor
from html.parser import HTMLParser
from urllib.parse import urljoin, urlsplit
from urllib.request import Request, urlopen


class Head(HTMLParser):
    def __init__(self):
        super().__init__()
        self.meta = {}

    def handle_starttag(self, tag, attrs):
        if tag == "meta":
            attrs = dict(attrs)
            self.meta[attrs.get("property", attrs.get("name"))] = attrs.get("content")


def fetch(url):
    request = Request(url, headers={"User-Agent": "OpenAI File Downloader, XaiImageApiFetch/1.0"})
    with urlopen(request, timeout=45) as response:
        return response.headers.get_content_type(), response.read()


def check(path):
    media, html = fetch(urljoin(origin, path))
    assert media == "text/html", (path, media)
    head = Head()
    head.feed(html.decode())
    assert head.meta["twitter:card"] == "summary_large_image", path
    assert head.meta["og:title"] and head.meta["og:description"], path
    assert (head.meta["og:image:width"], head.meta["og:image:height"]) == ("1200", "630"), path
    hashes = []
    for network, height in (("og", 630), ("twitter", 600)):
        url = head.meta[network + ":image"]
        assert head.meta[network + ":image:alt"], (network, path)
        assert urlsplit(url).netloc == urlsplit(origin).netloc, (path, url)
        media, png = fetch(url)
        assert media == "image/png" and png[:8] == b"\x89PNG\r\n\x1a\n", (url, media)
        assert struct.unpack(">II", png[16:24]) == (1200, height), url
        digest = hashlib.sha256(png).hexdigest()
        location = urlsplit(url)
        assert digest[:16] in location.path or location.query, (path, "missing image version")
        hashes.append(digest)
    return hashes


if __name__ == "__main__":
    origin = sys.argv[1]
    paths = ["/", "/wardrobe/sundress/", "/wardrobe/hussar/", "/experiments/strip/", "/experiments/cylinder/"]
    paths += ["/sections/" + s + "/" for s in ("wardrobe","studio","draft","cut","stitch","materials","studies","tracing","films","tools")]
    paths += ["/workbench/" + s for s in ("index.html","trace.html","materials.html","photo.html")]
    with ThreadPoolExecutor(max_workers=4) as workers:
        hashes = [digest for result in workers.map(check, paths) for digest in result]
    assert len(set(hashes)) == len(hashes), "Two destinations returned identical artwork"
    print(f"Checked {len(paths)} shared links and {len(hashes)} distinct PNG cards.")
