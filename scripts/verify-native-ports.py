"""Compare reviewed original plugins with native ports on controlled HTTP data.

These are synthetic protocol fixtures, not evidence of live source availability.
Reviewed main and public community plugins run in a temporary assets copy.
"""
import argparse
import base64
import codecs
import hashlib
import json
import os
import re
import shutil
import subprocess
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "verification/native-ports"
HASH = "1234567890abcdef1234567890abcdef12345678"
MAGNET = f"magnet:?xt=urn:btih:{HASH}"
TITLE = "Fixture 中文 ~ * | release"
FIXTURES = {
    "torrentscsv": json.dumps({"torrents": [dict(name=TITLE, infohash=HASH, size_bytes=1536,
                                                 seeders=12, leechers=3, created_unix=1700000000)]}),
    "piratebay": json.dumps([dict(name=TITLE, info_hash=HASH, id="42", size="1536", seeders="12",
                                  leechers="3", added="1700000000"),
                                 dict(info_hash="0" * 40)]),
    "eztv": f'''<table><tr class="forum_header_border" name="hover"><td><a class="epinfo" href="/ep/42" title="{TITLE} (1.50 KB)">title</a><a class="magnet" href="{MAGNET}">magnet</a></td><td>1.5 KB</td><td>unknown</td><td>12</td></tr></table>''',
    "torlock": f'''<article><table><tr><td><a href="/torrent/42/fixture.html">{TITLE}</a></td><td class="td">10/16/2026</td><td class="ts">1.5 KB</td><td class="tul">12</td><td class="tdl">3</td></tr><tr><td><a rel="nofollow" href="/torrent/43/ad.html">ad</a></td></tr></table></article>''',
    "solidtorrents": f'''<div class="search-result"><h5 class="title"><a href="/torrent/42">{TITLE}</a></h5><div class="stats"><div>type</div><div>1.5 KB</div><div><font>12</font></div><div><font>3</font></div><div>Oct 16, 2026</div></div><a class="dl-magnet" href="{MAGNET}">magnet</a></div>''',
    "torrentproject": f'''<div id="similarfiles"><div><span><a href="/42/fixture">{TITLE}</a></span><span>x</span><span>12</span><span>3</span><span>2026-10-16 00:00:00</span><span>1.5 KB</span></div></div><div id="nav"></div>''',
    "jackett": f'''<?xml version="1.0"?><rss xmlns:torznab="http://torznab.com/schemas/2015/feed"><channel><item><title>{TITLE}</title><jackettindexer>fixture</jackettindexer><link>https://example.org/file.torrent</link><comments>https://example.org/details/42</comments><size>1536</size><pubDate>Fri, 16 Oct 2026 00:00:00 +0000</pubDate><torznab:attr name="magneturl" value="{MAGNET}"/><torznab:attr name="seeders" value="12"/><torznab:attr name="peers" value="15"/></item></channel></rss>''',
    "nyaasi": f'''<table><tr><td>category</td><td><a href="/view/42" title="{TITLE}">title</a></td><td><a href="/download/42.torrent">file</a><a href="{MAGNET}">magnet</a></td><td>1.5 KB</td><td data-timestamp="1700000000">date</td><td>12</td><td>3</td><td>99</td></tr></table>''',
    "acgrip": f'''<table><tr><td><time datetime="1700000000">date</time></td><td><a href="/t/42">{TITLE}</a></td><td><a href="/download/42.torrent">file</a></td><td>1.5 KB</td></tr></table>''',
    "animetosho": json.dumps([dict(title=TITLE, magnet_uri=MAGNET, torrent_url="https://example.org/file.torrent", total_size=1536, seeders=12, leechers=3, link="https://example.org/details/42", timestamp=1700000000)]),
    "cloudtorrents": json.dumps(dict(next=None, results=[dict(id=42, torrent=dict(name=TITLE, torrentMagnet=MAGNET, size=1536, seeders=12, leechers=3, uploadedAt="2026-10-16T00:00:00Z", torrentType=dict(name="Movie")))])),
    "mikan": f'''<?xml version="1.0"?><rss><channel><item><title>{TITLE}</title><link>https://example.org/details/42</link><enclosure url="https://example.org/file.torrent" length="1536" /></item></channel></rss>''',
    "subsplease": json.dumps({"fixture": dict(release_date="Fri, 16 Oct 2026 00:00:00 +0000", downloads=[dict(res="1080", magnet=MAGNET+"&xl=1536")])}),
}
FIXTURES["sukebeisi"] = FIXTURES["nyaasi"]
FIXTURES["animetosho_community_006"] = FIXTURES["animetosho"]
FIXTURES["thepiratebay"] = json.dumps(json.loads(FIXTURES["piratebay"])[:1])
FIXTURES["torrentclaw"] = json.dumps(dict(results=[dict(title=TITLE, contentUrl="/content/42", torrents=[dict(infoHash=HASH, rawTitle=TITLE, sizeBytes=1536, seeders=12, leechers=3, uploadedAt="2026-10-16T00:00:00Z", qualityScore=90, scanStatus="success")])]))
FIXTURES["nekobt"] = json.dumps(dict(data=[dict(id=42, title=TITLE, filesize=1536, seeders="12", leechers="3", uploaded_at="1700000000000", infohash=HASH)]))
FIXTURES["tsukihime"] = json.dumps(dict(results=[dict(id=42, btih=HASH, name=TITLE, totalsize=1536, source_date=1700000000)]))
FIXTURES["goggames"] = json.dumps([dict(title=TITLE, infohash=HASH, slug="fixture", torrent_date="2026-10-16T00:00:00.000")])
FIXTURES["yts"] = json.dumps(dict(status="ok", status_message="fixture", data=dict(movie_count=1, limit=20, page_number=1, movies=[dict(id=42,url="https://example.org/movie/42",title=TITLE,torrents=[dict(url="https://example.org/file.torrent",seeds=12,peers=3,size_bytes=1536,size="1.5 KB",quality="1080p",video_codec="x265",type="web",audio_channels="5.1",date_uploaded_unix=1700000000)])])))
FIXTURES["snowfl"] = json.dumps([dict(name=TITLE, magnet=MAGNET, size="1.5 KB", seeder=12, leecher=3, url="https://example.org/details/42", site="fixture", age="2 days")])
FIXTURES["snowfl_community_052"] = FIXTURES["snowfl"]
FIXTURES["btdig"] = f'''<div class="one_result"><div class="torrent_name"><a href="https://example.org/details/42">{TITLE}</a></div><span class="torrent_size">1.5 KB</span><a href="{MAGNET}">magnet</a></div>'''
FIXTURES["anidex"] = f'''<p>Showing 1-1 of 1 torrents</p><table><tr><th>header</th></tr><tr><td><a class="torrent" href="/torrent/42"><span class="span-1440" title="{TITLE}">title</span></a><a href="{MAGNET}">magnet</a></td><td class="text-center td-992">1.5 KB</td><td class="text-success text-right">12</td><td class="text-danger text-right">3</td></tr></table>'''
FIXTURES["dmhy"] = f'''<table id="topic_list"><tbody><tr><td>2026/10/16</td><td>anime</td><td><a href="/topics/view/42">{TITLE}</a></td><td><a href="/file.torrent">file</a><a href="{MAGNET}">magnet</a></td><td>1.5 KB</td><td>12</td><td>3</td></tr></tbody></table>'''
FIXTURES["dmhy_community_018"] = FIXTURES["dmhy"]
FIXTURES["mikanani"] = f'''<table><tr><td><a class="magnet-link" target="_blank" href="/Home/Episode/42">{TITLE}</a></td><td>1.5 KB</td><td>date</td><td><a href="/download/42.torrent">file</a></td></tr></table>'''
FIXTURES["nyaapantsu"] = f'''<table><tr class="torrent-info"><td><a href="/view/42">{TITLE}</a><a href="{MAGNET}">magnet</a></td><td class="tr-se">12</td><td class="tr-le">3</td><td class="tr-size">1.5 KB</td><td class="tr-date">date</td></tr></table>'''
FIXTURES["nyaa"] = FIXTURES["nyaasi"].replace("1.5 KB", "1.5 MiB").replace("<table>","<table><tbody>").replace("</table>","</tbody></table>")+'<div class="pagination-page-info">Displaying results 1-1 out of 1 results</div>'
FIXTURES["torrentgalaxy"] = f'''<span class="steelblue">1</span><div class="tgxtablerow"><div class="tgxtablecell"><a class="txlight" title="{TITLE}" href="/torrent/42">title</a><a role="button" href="{MAGNET}">magnet</a></div><div class="tgxtablecell"><span class="badge badge-secondary">1.5 KB</span><font color="green">12</font><font color="#ff0000">3</font><div style="text-align:right"><small>16/10/26 00:00</small></div></div>''' + '<div class="tgxtablecell"></div>'*10 + '<div class="tgxtablecell"><small>end</small></div></div>'
FIXTURES["pantsu"] = json.dumps(dict(totalRecordCount=1,torrents=[dict(id=42,magnet=MAGNET,name=TITLE,filesize=1536,seeders=12,leechers=3)]))
FIXTURES["sukebei"] = FIXTURES["pantsu"]
FIXTURES["uindex"] = f'''<table><tr><td>type</td><td><a href="{MAGNET}">{TITLE}</a><a href="/details.php?id=42"></a></td><td>1.5 KB</td><td>date</td><td><span class="sr-seed">12</span></td><td><span class="sr-leech">3</span></td></tr></table>'''
FIXTURES["uniondht"] = f'''<p class="floatR">Found: 1 (fixture)</p><table><tr id="tor42"><td><a href="/topic/42"><b>{TITLE}</b></a></td><td><a href="/dl.php?id=42">1.5 KB</a></td><td class="seed">12</td><td class="leech"><b>3</b></td></tr></table>'''
for name in ("dodi_repacks", "onlinefix"):
    FIXTURES[name] = json.dumps(dict(downloads=[dict(title=TITLE, uploadDate="2026-10-16T00:00:00.000Z", fileSize="1.5 KB", uris=[MAGNET])]))
FIXTURES["smallgames"] = f'<a title="中文 {TITLE}" href="/?go=game&i=42">Скачать игру (1.5 Мб)</a>'
SPANISH_CARD = {"divxtotal", "dontorrent", "esmeraldatorrent", "naranjatorrent", "tomadivx"}
for name in SPANISH_CARD:
    FIXTURES[name] = f'''<p class="count">Se han encontrado <b>1</b> resultados </p><div id="buscador"><div class="card"><div class="card-body"><p><span><a href="detail/fixture">{TITLE}</a><span>Movie</span><span class="badge">HD</span></span></p></div></div></div>'''
PUBLIC_TABLES = {"glotorrents", "kickasstorrents", "pirateiro", "rockbox", "torrentdownload", "yourbittorrent", "magnetdl", "magnetdl_community_031", "solidtorrents_community_053"}


CATALOGUE_TABLES = {"bitsearch", "therarbg", "torrentdownloads", "traht", "xxxclubto", "bt4gprx", "linuxtracker", "sktorrent", "tokyotoshokan"}

def catalogue_fixture(source, path, address):
    if "/detail/42" in path or "/torrent/42" in path:
        return f'<a href="{MAGNET}">magnet</a>'
    if source == "bitsearch":
        return f'<p>Found <span>1</span></p><main class="mx-auto"><div class="space-y-4"><div class="bg-white"><div class="items-start"><div class="flex-1"><div class="items-center"><a href="/detail/42">{TITLE}</a></div><div class="items-center"><span>category</span><span>1.5 KB</span><span>10/16/2026</span></div><div class="items-center"><span class="font-medium">12</span><span class="font-medium">3</span></div></div><div class="space-y-2"><a href="{MAGNET}">magnet</a></div></div></div></div></main>'
    if source == "therarbg":
        return f'<title>Search for fixture</title><table><tbody><tr><td>type</td><td><a href="detail/42">{TITLE}</a></td><td><a>Movie</a></td><td>x</td><td>x</td><td>1,5 KB</td><td>12</td><td>3</td></tr></tbody></table>'
    if source == "torrentdownloads":
        return f'<div class="inner_container"><div class="grey_bar3"><span>x</span><span>3</span><span>12</span><span>1.5 KB</span><p><a href="/torrent/42">{TITLE}</a></p></div></div>'
    if source == "traht":
        return f'<div class="paginator_pages">Page 1 of 1</div><table id="releases-table"><tbody id="highlighted"><tr><td>type</td><td><a href="detail/42"><b>{TITLE}</b></a></td><td><a href="download.php?id=42">file</a></td><td>x</td><td>1,5 KB</td><td>12|3</td><td>x</td></tr></tbody></table>'
    if source == "xxxclubto":
        return f'<div class="browsetableinside"><ul><li>heading</li><li><span class="catlabe">type</span><span><a href="/detail/42">{TITLE}</a></span><span>date</span><span>1.5 KB</span><span>12</span><span>3</span></li></ul></div>'
    if source == "bt4gprx":
        if path == "/bt-detail/42":
            return f'<a href="//downloadtorrentfile.com/hash/{HASH}?name=fixture">file</a>'
        if path == "/trackerlist":
            return json.dumps(["udp://example.org:80/announce"])
        if path.endswith("/2"):
            return "<html></html>"
        return f'<div class="container"><div><a title="{TITLE}" href="/bt-detail/42">title</a><b class="cpill">1.5 KB</b><b id="seeders">12</b><b id="leechers">3</b></div></div>'
    if source == "sktorrent":
        if "page=1" in path:
            return "Nenasli ste co ste hladali"
        return f'<A href="details.php?id={HASH}"><br>{TITLE}</A><br>Velkost 1.5 KB | Pridany 16/10/2026<br>Odosielaju : 12<br>Stahuju : 3'
    if source == "linuxtracker":
        return f'<a href="index.php?page=torrent-details&id=42" title="detail">{TITLE}</a><strong>x</strong>x<strong>x</strong>x<strong>x</strong>1.5 KB<strong>x</strong>12<strong>x</strong>3<strong>x</strong>x<a href="{MAGNET}">magnet</a>'
    if source == "tokyotoshokan":
        return f'<table class="listing"><tr class="shade category_1"><td><a href="file.torrent" type="application/x-bittorrent">{TITLE}</a><a href="{MAGNET}">magnet</a><a href="details.php?id=42">details</a></td><td class="desc-bot">Size: 1.5KB more</td><td class="stats"><span>12</span><span>3</span></td></tr></table>'
    raise AssertionError(source)


LEGACY_MEDIA = {"cpasbien", "torrent9", "dontorrent_community_020", "elitetorrent", "maxitorrent", "mejortorrent", "pediatorrent_community_043"}

def legacy_fixture(source,path,address,category):
    if path == "/domain":
        return address if source == "cpasbien" else json.dumps(dict(torrent9=[address]))
    if source == "cpasbien":
        if path.endswith("/51"):
            return "<html></html>"
        return f'<table class="table-corps"><tr><td><a class="titre" href="/detail/42">{TITLE}</a></td><td><div class="poid">1.5 Ko</div></td><td><div class="up">12</div></td><td><div class="down">3</div></td></tr></table>'
    if source == "torrent9":
        if path.endswith("page-2"):
            return "<html></html>"
        return f'<table><tbody><tr><td><a href="/detail/42"><h3>{TITLE}</h3></a></td><td>x</td><td>x</td><td>x</td><td>1.5 Ko</td><td>x</td><td>12</td><td>x</td><td>3</td></tr></tbody></table>'
    if source == "dontorrent_community_020":
        if path.startswith("/pelicula/"):
            return '<p class="mb-0"><b>size</b>1,5 KB</p><a class="text-white bg-primary rounded-pill d-block shadow text-decoration-none p-1" href="//files.example.org/42.torrent">file</a>'
        return '<p class="lead">header</p><p class="lead"><b>1</b></p><a class="text-decoration-none" href="/pelicula/fixture-release">result</a>'
    if source == "pediatorrent_community_043":
        if path.startswith("/peliculas/"):
            return '<a href="#">skip</a>'*11 + '<a href="/42.torrent">file</a>'
        return f'<p class="text-2xl text-lime-500 text-center">Showing 1-1 of 1</p><a href="{address}/peliculas/42-fixture-release">result</a>'
    if source == "mejortorrent":
        kind = "serie" if category == "tv" else "pelicula"
        if path.startswith("/busqueda"):
            return f'<a href="{address}/{kind}/42/fixture">result</a>'
        if kind == "serie":
            return f'<p class="text-blue-900">{TITLE}</p><table><tbody><tr><td>42</td><td>1-2</td><td>2026-10-16</td><td><p>Sin clave</p><a href="/42.torrent">file</a></td></tr></tbody></table>'
        return f'<p class="text-blue-900">{TITLE}</p><a href="/quality/1080p">quality</a><a href="/torrents/42.torrent">file</a><p>16/10/2026</p>'
    if source == "elitetorrent":
        kind = "series" if category == "tv" else "peliculas"
        if path.startswith(("/?s=","/page/")):
            return f'Resultado de buscar<a href="{address}/{kind}/fixture/">result</a>'
        encoded = base64.b64encode(codecs.encode(MAGNET,"rot_13").encode()).decode()
        return f'<h1>Descargar Fixture release por torrent</h1><a href="?i={encoded}">skip</a><a href="?i={encoded}">file</a>Tamaño:</b> 1.5 MB Calidad:</b> 1080p Idioma:</b> English <p>Fecha:</b> 2026-10-16</p><b>Semillas</b>: 12<b>Clientes</b>: 3'
    if source == "maxitorrent":
        if path == "/get/result/":
            return json.dumps(dict(data=dict(torrents={"group":{"row":dict(torrentSize="1.5 KB",guid="detail/42")}} if Replay.post_body.endswith("pg=1") else {"end":None})))
        return 'window.location.href = "//xx/files/fixture.torrent";'
    raise AssertionError(source)


def table_fixture(source, category, address):
    if source == "glotorrents":
        return f'''<table><tr class='t-row'> <td class='ttable_col1'><a title="{TITLE}" href="details/42">title</a></td><td align='center'><a href="{MAGNET}">magnet</a></td><td>1.5 KB</td><td><font color='green'><b>12</b></font></td><td><font color='#112233'><b>3</b></font></td></tr></table>'''
    if source == "kickasstorrents":
        return f'''<table><tr class="odd"><td><div class="torrentname"><a href="detail/fixture" class="cellMainLink">{TITLE}</a></div></td><td>1.5 KB</td><td class="green center">12</td><td class="red lasttd center">3</td></tr></table>'''
    if source == "pirateiro":
        return f'''<table><tr><td><a href="{address}/torrent/42"><h6 class="title">{TITLE}</h6></a><span class="btn-seed-home">12</span><span class="btn-leech-home">3</span></td></tr></table>'''
    if source == "rockbox":
        return f'''<table><TR> <td align="center"><a HREF="details.php?id=42" title="details: {TITLE}">title</a><a HREF=download.php?id=42>file</a></td><td class="lista">16/10/2026</td><td>1.5 KB</td><td><a title="peers details">12</a></td><td><a title="peers details">3</a></td></TR></table>'''
    if source == "torrentdownload":
        label = dict(all="Movie", movies="Movie", tv="Television", anime="Anime", books="Book", games="Game", music="Music", software="Software")[category]
        return f'''<h1>1 - 1 of 1 for fixture</h1><table><tr><td class="tt-name"><a href="/{HASH}/fixture">{TITLE}</a><span class="smallish"> » {label}</span></td><td class="tdnormal">1</td><td class="tdnormal">1.5 KB</td><td class="tdseed">12</td><td class="tdleech">3</td></tr></table>'''
    if source == "yourbittorrent":
        return f'''<div class="table-responsive"><table></table></div><div class="table-responsive"><table><tr class="table-default"><td><a href="detail/fixture" title="{TITLE}">title</a></td><td>1.5 kB</td><td class="sd">12</td><td class="pr">3</td></tr></table></div>'''
    if source.startswith("magnetdl"):
        kind = dict(all="Movie", books="E-Book", games="Game", movies="Movie", music="Music", software="Software", tv="TV")[category]
        return f'''<table><tr><td class="m"><a href="{MAGNET}" title="Direct Download" rel="nofollow"><img src="/img/m.gif" alt="Magnet Link" width="16" height="16" /></a></td><td class="n"><a href="/file/42/" title="{TITLE}">{TITLE}</a></td><td>today</td><td class="t1">{kind}</td><td>1</td><td>1.5 KB</td><td class="s">12</td><td class="l">3</td></tr></table><div id="footer">Found <strong>1</strong> Magnet Links for <i>fixture</i></div>'''
    if source == "solidtorrents_community_053":
        return f'''<b>1</b><ul><li class="search-result"><div class="info"><h5><a href="/torrent/42">{TITLE}</a></h5><div class="stats"><div>type</div><div>1.5 KB</div><div>12</div><div>3</div></div></div><div class="links"><a class="dl-magnet" href="{MAGNET}">magnet</a></div></li></ul>'''
    raise AssertionError(source)


class Replay(BaseHTTPRequestHandler):
    source = ""
    requests = []
    mode = "normal"
    category = "all"

    def log_message(self, *_):
        pass

    def do_POST(self):
        type(self).post_body = self.rfile.read(int(self.headers.get("Content-Length", 0))).decode()
        self.do_GET()

    def do_GET(self):
        type(self).requests.append(self.path)
        type(self).request_contracts.append([self.command,self.path,self.post_body if self.command=="POST" else ""])
        if self.mode == "delayed":
            time.sleep(1.5)
        if self.mode == "failed" or (self.mode == "paged_failure" and "/seeds/2/" in self.path):
            self.send_error(503)
            return
        if self.mode == "resolve":
            if self.path=="/domain":
                address=f"http://127.0.0.1:{self.server.server_port}"
                payload=(address if self.source=="cpasbien" else json.dumps(dict(torrent9=[address]))).encode()
            elif self.source=="thepiratebay_community_059":
                payload=json.dumps(dict(name=TITLE,info_hash=HASH)).encode()
            elif self.source=="cpasbien":
                payload=b'<a href="/get_torrent/42">torrent</a>'
            elif self.source=="torrent9":
                payload=b'<a class="btn btn-danger download" href="/get/42">torrent</a>'
            else:
                payload=f'<a href="{MAGNET}">magnet</a>'.encode()
        elif self.mode == "malformed":
            payload = b"not valid JSON"
        elif self.mode == "empty":
            payload = b'{"torrents": []}'
        elif self.mode == "challenge":
            payload = b"<html><title>Just a moment...</title>cf-chl-test</html>"
        elif self.source == "ali213":
            address = f"http://127.0.0.1:{self.server.server_port}"
            if self.path.startswith("/pcgame/"):
                payload = b'var downUrl ="/soft50/42"'
            elif self.path == "/soft50/42":
                payload = f'<a class="result_js" href="{address}/soft5566/42" target="_blank">file</a>'.encode()
            elif self.path == "/soft5566/42":
                payload = f'<a id="btbtn" href="{address}/y/fixture.torrent" target="_blank">file</a>'.encode()
            else:
                payload = f'<p class="downAddress"><a href="{address}/pcgame/42.html" target="_blank">game<em>1.5 GB</em></a></p>'.encode()
        elif self.source == "fitgirl_repacks":
            address = f"http://127.0.0.1:{self.server.server_port}"
            payload = f'<article><h2 class="entry-title"><a href="{address}/post/42">{TITLE}</a></h2><time class="entry-date" datetime="2026-10-16T00:00:00Z">date</time><p>Repack Size: 1.5 GB</p><a href="{MAGNET}">magnet</a></article>'.encode()
        elif self.source == "rutor":
            payload = f'<b>Found</b> Результатов поиска 1 <table><tr class="gai"><td>16 Окт 26</td><td><a href="{MAGNET}">magnet</a><a href="/torrent/42/fixture">{TITLE}</a></td><td align="right">1.5&nbsp;KB</td><td><span class="green">12</span> <span class="red">3</span></td></tr></table>'.encode()
        elif self.source == "mypornclub":
            if self.path == "/detail/42":
                payload = f'<a href="{MAGNET}">magnet</a>'.encode()
            else:
                payload = f'<div>Page 1 of 1</div><div class="torrents_list"><div class="torrent_element"><div class="torrent_element_text_div"><span class="torrent_element_text_span"><a href="/detail/42">{TITLE}</a>fixture</span></div><div class="torrent_element_info"><a class="linkadd">2 days ago</a><span class="teis">[size]:</span>1,5 KB<span class="teis">[seeders]:</span>12<span class="teis">[leechers]:</span>3</div></div></div>'.encode()
        elif self.source == "zooqle":
            if self.path == "/torrent-page/":
                category = dict(all="movies",software="apps",books="books").get(self.category,self.category)
                def info(icon,label,value):
                    return f'<li><i class="fa {icon}"></i>&nbsp;<strong>{label}</strong></li><li>:</li><li>{value}</li>'
                payload = (f'<title>{TITLE} Torrent - Zooqle</title><input type="hidden" name="hid" value="fixture-hid">' + info("fa-file","Size","1.5 KB")+info("fa-arrow-up","Seed","12")+info("fa-arrow-down","Leech","3")+info("fa-tag","Category",f'<a>{category}</a>')).encode()
            else:
                payload = b'<table><tr><td><input type="hidden" name="id" value="42"></td></tr></table>'
        elif self.source in LEGACY_MEDIA:
            payload = legacy_fixture(self.source,self.path,f"http://127.0.0.1:{self.server.server_port}",self.category).encode()
        elif self.source == "academictorrents":
            address = f"http://127.0.0.1:{self.server.server_port}"
            if self.path.endswith("/tech"):
                payload = b'<table><tr><td>Mirrors</td><td>12 complete, 3 downloading</td></tr><tr><td>Added</td><td>2026-10-16T00:00:00</td></tr></table>'
            else:
                payload = f'<rss><channel><item><title>{TITLE}</title><description>fixture</description><infohash>{HASH}</infohash><size>1536</size><link>{address}/details/42</link></item></channel></rss>'.encode()
        elif self.source == "audiobookbay":
            if "/page/" in self.path:
                payload = f'<div class="post"><h2 class="postTitle"><a href="detail/42">{TITLE}</a></h2></div>'.encode()
            elif "/detail/42" in self.path:
                payload = f'<table><tr><td>Info Hash:</td><td>{HASH}</td></tr><tr><td>Combined File Size:</td><td>1.5 <span>KBs</span></td></tr></table>'.encode()
            else:
                payload = b'<html>healthy mirror</html>'
        elif self.source == "darklibria":
            address = f"http://127.0.0.1:{self.server.server_port}"
            if "/serial/42" in self.path:
                payload = f'<h1 id="russian_name">{TITLE}</h1><table><tr class="torrent"><td>TV</td><td>1080p</td><td>1.5 Кб</td><td>2026-10-16 00:00:00</td><td><a title="Magnet-ссылка" href="{MAGNET}">magnet</a></td><td>12</td><td>3</td></tr></table>'.encode()
            else:
                payload = f'<span class="text text-light mt-0">Showing items 1-1 of 1</span><table><tbody style="vertical-align: center"><tr><td><a href="{address}/serial/42">series</a></td></tr></tbody></table>'.encode()
        elif self.source == "eztvx":
            if "apikey=" in self.path:
                payload = b'{"Response":"False"}'
            else:
                payload = json.dumps(dict(torrents_count=1,torrents=[dict(title=TITLE,magnet_url=MAGNET,size_bytes=1536,seeds=12,peers=3,episode_url="https://example.org/episode/42")])).encode()
        elif self.source == "thepiratebay_community_059":
            payload = FIXTURES["thepiratebay"].encode()
        elif self.source == "torrentdownload_community_066":
            kind = dict(all="Movie",anime="Anime",books="Book",games="Game",movies="Movie",music="Music",pictures="Picture",software="Software",tv="TV")[self.category]
            payload = f'<table><tr><td class="tdleft"><div class="tt-name"><a href="/{HASH}/fixture">{TITLE}</a><span class="smallish"> » {kind}</span></div></td><td class="tdnormal">1.5 KB</td><td class="tdseed">12</td><td class="tdleech">3</td></tr></table>'.encode() if "p=1" in self.path else b'<table></table>'
        elif self.source in CATALOGUE_TABLES:
            payload = catalogue_fixture(self.source, self.path, f"http://127.0.0.1:{self.server.server_port}").encode()
        elif self.source in {"apachetorrent", "redetorrent", "torrenflix"}:
            address = f"http://127.0.0.1:{self.server.server_port}"
            if "/baixar-torrent/42" in self.path:
                if self.source == "torrenflix":
                    payload = f'<div class="download-links"><table><tbody><tr><td>Server<span>badge</span></td><td>EN</td><td>1080p</td><td><a href="{MAGNET}">download</a></td></tr></tbody></table></div>'.encode()
                else:
                    area = 'div id="lista_links"' if self.source == "apachetorrent" else 'p id="lista_download"'
                    tag = area.split()[0]
                    payload = f'<{area}><a href="{MAGNET}" title="1080p">magnet</a></{tag}>'.encode()
            elif self.source == "apachetorrent":
                payload = f'<div class="capaname"><a href="{address}/baixar-torrent/42" title="{TITLE}">{TITLE} Download</a></div>'.encode()
            elif self.source == "redetorrent":
                payload = f'<div class="capa_lista"><a href="{address}/baixar-torrent/42"><h2 itemprop="headline">{TITLE} Download</h2></a></div>'.encode()
            else:
                payload = f'<main><ul class="post-lst"><li><a class="lnk-blk" href="{address}/baixar-torrent/42"></a><h2>{TITLE}</h2></li></ul></main>'.encode()
        elif self.source == "yggtracker":
            address = f"http://127.0.0.1:{self.server.server_port}"
            if self.path == "/nodes.json":
                payload = json.dumps([dict(url=address+"/tracker?%s", categories={category: [] for category in ("all","anime","books","games","movies","music","pictures","software","tv")})]).encode()
            else:
                payload = json.dumps(dict(tracker=dict(url=address),torrents=[dict(torrent=dict(url="https://example.org/details/42",file=dict(url="https://example.org/file.torrent",name=TITLE,size=1536),scrape=dict(seeders=12,leechers=3)))])).encode()
        elif self.source == "tsukihime" and self.path.startswith("/v1/torrents/"):
            payload = json.dumps(dict(trackers=[dict(url="udp://example.org:80/announce",seeders=12,leechers=3)])).encode()
        elif self.source in {"snowfl", "snowfl_community_052"}:
            if self.path in {"/", "/index.html"}:
                payload = b'<script src="b.min.js?fixture=1"></script>'
            elif self.path.startswith("/b.min.js"):
                payload = b'findNextItem("fixturekey");"fixturekey";$((function(){var e,t,n,r,o,a,i='
            else:
                payload = FIXTURES[self.source].encode()
        elif self.source in PUBLIC_TABLES:
            if "/detail/fixture" in self.path:
                payload = f'<a href="{MAGNET}">magnet</a>'.encode()
            elif (self.source in {"glotorrents", "rockbox"} and "page=1" in self.path
                  or self.source == "kickasstorrents" and self.path.endswith("/1/")
                  or self.source == "pirateiro" and "page=2" in self.path):
                payload = b"<table></table>"
            else:
                payload = table_fixture(self.source, self.category, f"http://127.0.0.1:{self.server.server_port}").encode()
        elif self.source == "subsplease" and self.path.endswith("&p=1"):
            payload = b"{}"
        elif self.source in SPANISH_CARD | {"calidadtorrent", "pediatorrent"}:
            if self.path.endswith("/detail/fixture"):
                download = "//files.example.org/torrents/42.torrent" if self.source in SPANISH_CARD else "/torrents/42.torrent"
                payload = f'''<h1 class='title'>{TITLE}</h1><a href='{download}'>file</a><p class='size'><b class='label'>Tamaño:</b> 1,5 KB</p>'''.encode()
            elif self.source in SPANISH_CARD:
                payload = FIXTURES[self.source].replace('href="detail/fixture"', 'href="/detail/fixture"' if self.source == "dontorrent" else 'href="detail/fixture"').encode()
            elif "/page/2" in self.path:
                payload = b"<p>No se ha encontrado ningun resultado.</p>"
            else:
                layout = "movie-list serie-list doc-list" if self.source == "pediatorrent" else "result-list"
                detail = "false" if self.source == "pediatorrent" else "true"
                payload = f'''<div class="{layout}"><div x-data="{{ showDetail: {detail} }}"><div class="relative"><a href="http://127.0.0.1:{self.server.server_port}/detail/fixture">result</a></div></div></div>'''.encode()
        elif self.source == "limetorrents":
            page = int(self.path.rstrip("/").rsplit("/", 1)[1])
            payload = (ROOT / f"verification/limetorrents-benchmark/fixtures/nirvana-in-fire-{min(page, 2)}.html").read_bytes()
        elif self.path.endswith("/42/fixture"):
            payload = f'<a href="{MAGNET}">magnet</a>'.encode()
        else:
            payload = FIXTURES[self.source].encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        try:
            self.wfile.write(payload)
        except (BrokenPipeError, ConnectionResetError, ConnectionAbortedError):
            pass  # The deadline test intentionally closes the request early.


def canonical(report):
    return sorted((row["reference"]["uri"], row["title"], origin["size_bytes"], origin["seeders"],
                   origin["leechers"], origin["description_url"], json.dumps(row["reference"].get("post_form"), sort_keys=True), origin["published_unix"])
                  for row in report["results"] for origin in row["observations"])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--executable", type=Path, default=ROOT / ".cache/native-ports-target/debug/torrent-search.exe")
    parser.add_argument("--python", type=Path, default=ROOT / ".cache/python-3.14.8-embed-amd64/python.exe")
    parser.add_argument("--source", nargs="+")
    parser.add_argument("--report-name", default=None)
    parser.add_argument("--resolve-only", action="store_true")
    args = parser.parse_args()
    OUTPUT.mkdir(parents=True, exist_ok=True)
    server = ThreadingHTTPServer(("127.0.0.1", 0), Replay)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    address = f"http://127.0.0.1:{server.server_port}"
    records = []
    try:
        with tempfile.TemporaryDirectory(prefix="native-ports-", dir=ROOT / ".cache") as folder:
            assets = Path(folder)
            shutil.copytree(ROOT / "resources", assets, dirs_exist_ok=True)
            manifest = json.loads((assets / "sources.json").read_text())
            for source in manifest["sources"]:
                source["site_url"] = address
            # The original Pirate Bay plugin hardcodes a distinct API endpoint.
            # Redirect only that URL in the test copy and pin the resulting file.
            pirate = assets / "qbit/engines/piratebay.py"
            pirate.write_bytes(pirate.read_bytes().replace(b"https://apibay.org", address.encode()))
            next(s for s in manifest["sources"] if s["id"] == "piratebay")["sha256"] = hashlib.sha256(pirate.read_bytes()).hexdigest()
            for identifier, upstream in [("animetosho", "https://feed.animetosho.org"),
                                         ("animetosho_community_006", "https://feed.animetosho.xyz"),
                                         ("cloudtorrents", "https://api.cloudtorrents.com"),
                                         ("thepiratebay", "https://apibay.org"),
                                         ("subsplease", "https://subsplease.org")]:
                script = assets / f"qbit/engines/{identifier}.py"
                if script.exists():
                    script.write_bytes(script.read_bytes().replace(upstream.encode(), address.encode()))
                    next(s for s in manifest["sources"] if s["id"] == identifier)["sha256"] = hashlib.sha256(script.read_bytes()).hexdigest()
            for identifier, upstream in [("dodi_repacks", "https://hydralinks.cloud"),
                                         ("onlinefix", "https://hydralinks.cloud"),
                                         ("smallgames", "https://small-games.info"),
                                         ("tsukihime", "https://api.tsukihime.org"),
                                         ("yts", " https://movies-api.accel.li"),
                                         ("yggtracker", "https://raw.githubusercontent.com/YGGverse/qbittorrent-yggtracker-search-plugin/main/nodes.json"),
                                         ("snowfl_community_052", "https://snowfl.com")]:
                script = assets / f"qbit/engines/{identifier}.py"
                if script.exists():
                    replacement = address + "/nodes.json" if identifier == "yggtracker" else address
                    script.write_bytes(script.read_bytes().replace(upstream.encode(), replacement.encode()))
                    next(s for s in manifest["sources"] if s["id"] == identifier)["sha256"] = hashlib.sha256(script.read_bytes()).hexdigest()
            for identifier, upstream in [("bt4gprx", "https://downloadtorrentfile.com"), ("sktorrent", "https://sktorrent.eu"), ("dmhy_community_018", "http://dmhy.org"), ("torrentgalaxy", "https://torrentgalaxy.to"), ("nyaa", "https://sukebei.nyaa.si"), ("anidex", "https://anidex.info")]:
                script = assets / f"qbit/engines/{identifier}.py"
                if script.exists():
                    script.write_bytes(script.read_bytes().replace(upstream.encode(), address.encode()))
                    next(s for s in manifest["sources"] if s["id"] == identifier)["sha256"] = hashlib.sha256(script.read_bytes()).hexdigest()
            (assets / "sources.json").write_text(json.dumps(manifest))
            for identifier, replacements in {
                "academictorrents": [("https://academictorrents.com", address), ('home = str(Path.home())', 'home = ' + repr(str(assets / "home")))],
                "cpasbien": [("https://raw.githubusercontent.com/MarcBresson/cpasbien/master/cpasbien.url",address+"/domain")],
                "torrent9": [("https://raw.githubusercontent.com/menegop/qbfrench/master/urls.json",address+"/domain")],
                "audiobookbay": [("http://theaudiobookbay.se",address),("http://audiobookbay.fi",address),("http://audiobookbay.is",address)],
                "darklibria": [("https://darklibria.it",address)],
                "eztvx": [("http://www.omdbapi.com",address)],
                "thepiratebay_community_059": [("https://apibay.org",address)],
                "ali213": [("http://down.ali213.net",address),("http://www.soft50.com",address),("http://btfile.soft5566.com",address)],
                "rutor": [("https://rutor.info/",address+"/"), ('url.replace("//", "//d.")', "url")],
                "zooqle": [("https://zooqle.skin",address)],
                "mejortorrent": [("https://www36.mejortorrent.eu",address)],
                "fitgirl_repacks": [("https://fitgirl-repacks.site/",address+"/")],
            }.items():
                script = assets / f"qbit/engines/{identifier}.py"
                if script.exists():
                    content = script.read_bytes()
                    for before, after in replacements:
                        content = content.replace(before.encode(),after.encode())
                    script.write_bytes(content)
                    next(s for s in manifest["sources"] if s["id"] == identifier)["sha256"] = hashlib.sha256(content).hexdigest()
            (assets / "home/AppData/Roaming").mkdir(parents=True)
            (assets / "sources.json").write_text(json.dumps(manifest))
            (assets / "qbit/engines/jackett.json").write_text(json.dumps(dict(url=address, api_key="fixture-not-a-secret", tracker_first=False, thread_count=1)))

            def run(source, backend, category="all", limit=200, timeout=20):
                Replay.source = source
                Replay.category = category
                Replay.requests = []
                Replay.request_contracts = []
                command = [str(args.executable), "--assets", str(assets), "--python",
                           str(args.python if backend == "python" else assets / "nonexistent-python.exe"),
                           "--source-backend", backend, "--json", "search", "fixture" if source in {"uindex","eztvx"} else "fixture 中文 ~ *",
                           "--source", source, "--category", category, "--limit", str(limit), "--timeout", str(timeout)]
                process = subprocess.run(command, capture_output=True, timeout=30, encoding="utf-8", env={**os.environ, "TORRENT_SEARCH_CACHE_DIR": str(assets / "native-cache"), "FITGIRL_CACHE":"0", "FITGIRL_SCRAPE":"0"})
                if not process.stdout:
                    raise RuntimeError(process.stderr)
                report = json.loads(process.stdout)
                report["request_contracts"] = Replay.request_contracts[:]
                return process.returncode, report, Replay.requests[:]

            for source in manifest["sources"]:
                identifier = source["id"]
                if args.resolve_only:
                    continue
                if args.source and identifier not in args.source:
                    continue
                for category in source["categories"]:
                    pair = {backend: run(identifier, backend, category) for backend in ("python", "rust")}
                    if any(code != 0 or report["partial"] for code, report, _ in pair.values()):
                        raise AssertionError(f"Incomplete search: {identifier}/{category}: {pair}")
                    left, right = (canonical(pair[b][1]) for b in ("python", "rust"))
                    # Relative dates may differ by one second between processes.
                    if [row[:-1] for row in left] != [row[:-1] for row in right] or any(
                            (a[-1] != b[-1] and (a[-1] is None or b[-1] is None or abs(a[-1]-b[-1]) > 2))
                            for a, b in zip(left, right)):
                        (OUTPUT / "mismatch.json").write_text(json.dumps(dict(source=identifier, category=category, python=left, rust=right), ensure_ascii=False, indent=2), encoding="utf-8")
                        raise AssertionError(f"Parity mismatch: {identifier}/{category}")
                    expected = (0 if identifier=="zooqle" and category=="books" else 31 if identifier == "limetorrents" else
                                dict(anime=13,books=2,games=2,music=2,pictures=3).get(category,1)
                                if identifier == "thepiratebay_community_059" else 1)
                    if len(left) != expected:
                        raise AssertionError(f"Unexpected fixture count: {identifier}/{category}: {len(left)}")
                    def routes(backend):
                        values = pair[backend][2]
                        if identifier == "snowfl":
                            values = [re.sub(r"/[^/]{8}/0/SEED/NONE/1\?_=\d+$", "/CACHE/0/SEED/NONE/1?_=TIME", value) for value in values]
                        if identifier in {"cpasbien","torrent9"}:
                            values = [value for value in values if value != "/domain"]
                        return values
                    if routes("python") != routes("rust"):
                        raise AssertionError(f"Request route mismatch: {identifier}/{category}: {pair['python'][2]} != {pair['rust'][2]}")
                    if identifier == "zooqle" and pair["python"][1]["request_contracts"] != pair["rust"][1]["request_contracts"]:
                        raise AssertionError(f"POST contract mismatch: {identifier}/{category}")
                    records.append(dict(source=identifier, category=category, results=len(left), parity=True, routes=pair["rust"][2]))
                    print(f"{identifier}/{category}: parity and routes OK", flush=True)
                code, report, _ = run(identifier, "rust", limit=1)
                assert code == 0 and len(report["results"]) == 1 and report["sources"][0]["limited"]
                Replay.mode = "failed"
                code, report, _ = run(identifier, "rust")
                assert code != 0 and report["partial"] and report["sources"][0]["status"] == "failed"
                Replay.mode = "normal"
            resolver_records=[]
            def resolve(identifier,backend,path):
                Replay.source=identifier
                Replay.requests=[]
                Replay.request_contracts=[]
                command=[str(args.executable),"--assets",str(assets),"--python",str(args.python if backend=="python" else assets/"missing-python.exe"),"--source-backend",backend,"--json","resolve","--source",identifier,address+path]
                process=subprocess.run(command,capture_output=True,encoding="utf-8",timeout=25)
                if process.returncode:
                    raise AssertionError(f"Resolution failed {identifier}/{backend}: {process.stderr}")
                return json.loads(process.stdout)
            Replay.mode="resolve"
            for identifier,path in {
                "torrentproject":"/detail/42", "pirateiro":"/detail/42",
                "thepiratebay_community_059":"/description.php?id=42",
                "cpasbien":"/detail/42", "torrent9":"/detail/42",
                "rockbox":"/download.php?id=42", "traht":"/download.php?id=42&ok=",
                "smallgames":"/getTorrent.php?direct=1&gid=42", "rutor":"/download/42",
                "zooqle":"/torfile/#hid=fixture-hid",
                "animetosho":"/direct.torrent",
            }.items():
                if args.source and identifier not in args.source:
                    continue
                pair={backend:resolve(identifier,backend,path) for backend in ("python","rust")}
                assert pair["python"]==pair["rust"],(identifier,pair)
                resolver_records.append(dict(source=identifier,reference=pair["rust"],parity=True))
            Replay.mode = "paged_failure"
            code, report, _ = run("limetorrents", "rust")
            assert code != 0 and report["partial"] and len(report["results"]) == 31
            Replay.mode = "malformed"
            code, report, _ = run("torrentscsv", "rust")
            assert code != 0 and report["sources"][0]["status"] == "failed"
            Replay.mode = "empty"
            code, report, _ = run("torrentscsv", "rust")
            assert code == 0 and not report["results"] and not report["partial"]
            Replay.mode = "challenge"
            code, report, _ = run("torlock", "rust")
            assert code != 0 and report["sources"][0]["status"] == "failed"
            Replay.mode = "delayed"
            code, report, _ = run("torrentscsv", "rust", timeout=1)
            assert code != 0 and report["sources"][0]["status"] == "timed_out"
            Replay.mode = "normal"
            output_name = args.report_name or ("resolver-parity.json" if args.resolve_only else "controlled-parity.json" if not args.source else "controlled-parity-selected.json")
            (OUTPUT / output_name).write_text(json.dumps(dict(
                fixture_kind="synthetic protocol fixtures; LimeTorrents uses captured real HTML", records=records,
                no_python_native=True, result_limits_passed=True, http_errors_passed=True,
                resolver_records=resolver_records,
                partial_results_passed=True, malformed_json_passed=True, empty_success_passed=True,
                challenge_detection_passed=True, deadline_passed=True,
                executable_sha256=hashlib.sha256(args.executable.read_bytes()).hexdigest()), ensure_ascii=False, indent=2), encoding="utf-8")
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()
