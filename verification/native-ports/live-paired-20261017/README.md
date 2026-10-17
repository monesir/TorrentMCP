# Live search of all public plugins

Query: `nirvana in fire`. Category: all. Per-source timeout: 20s, limit: 20, parallel: 6.

Rust and Python ran sequentially against live websites. Network/cache/site changes affect comparisons; these are observations, not a controlled speed benchmark. Jackett has no local credentials. Completed with zero results does not prove that a site parser works.

Title matches are advisory checks for Nirvana in Fire / Lang Ya Bang / 琅琊榜, not full content validation. No torrent payload was downloaded.

| Source | Rust status | Results | Title matches | ms | Python status | Results | Title matches | ms |
|---|---|---:|---:|---:|---|---:|---:|---:|
| eztv | failed | 0 | 0 | 15 | failed | 0 | 0 | 836 |
| jackett | failed | 0 | 0 | 0 | completed | 1 | 1 | 880 |
| limetorrents | completed | 20 | 19 | 3290 | completed | 20 | 19 | 2113 |
| piratebay | completed | 3 | 3 | 806 | completed | 3 | 3 | 490 |
| solidtorrents | failed | 0 | 0 | 1993 | failed | 0 | 0 | 13376 |
| torlock | failed | 0 | 0 | 340 | failed | 0 | 0 | 482 |
| torrentproject | failed | 0 | 0 | 4808 | failed | 0 | 0 | 13301 |
| torrentscsv | completed | 0 | 0 | 303 | completed | 0 | 0 | 1131 |
| acgrip | completed | 0 | 0 | 564 | completed | 0 | 0 | 811 |
| animetosho | failed | 0 | 0 | 15 | failed | 0 | 0 | 550 |
| animetosho_community_006 | completed | 2 | 0 | 1508 | completed | 2 | 0 | 1287 |
| cloudtorrents | failed | 0 | 0 | 397 | failed | 0 | 0 | 475 |
| mikan | completed | 0 | 0 | 1807 | completed | 0 | 0 | 1539 |
| nyaasi | completed | 0 | 0 | 591 | completed | 0 | 0 | 506 |
| subsplease | completed | 0 | 0 | 603 | completed | 0 | 0 | 483 |
| sukebeisi | failed | 0 | 0 | 136 | failed | 0 | 0 | 333 |
| thepiratebay | completed | 3 | 3 | 615 | completed | 3 | 3 | 470 |
| torrentclaw | completed | 0 | 0 | 552 | completed | 0 | 0 | 473 |
| divxtotal | failed | 0 | 0 | 4905 | failed | 0 | 0 | 3980 |
| dontorrent | failed | 0 | 0 | 7260 | failed | 0 | 0 | 1983 |
| esmeraldatorrent | failed | 0 | 0 | 4086 | failed | 0 | 0 | 12351 |
| naranjatorrent | failed | 0 | 0 | 3169 | failed | 0 | 0 | 12366 |
| tomadivx | failed | 0 | 0 | 4527 | failed | 0 | 0 | 4871 |
| calidadtorrent | failed | 0 | 0 | 369 | failed | 0 | 0 | 481 |
| pediatorrent | failed | 0 | 0 | 359 | failed | 0 | 0 | 489 |
| glotorrents | failed | 0 | 0 | 4671 | failed | 0 | 0 | 12654 |
| kickasstorrents | failed | 0 | 0 | 1912 | failed | 0 | 0 | 450 |
| pirateiro | completed | 0 | 0 | 759 | completed | 0 | 0 | 728 |
| rockbox | failed | 0 | 0 | 8007 | failed | 0 | 0 | 12204 |
| torrentdownload | completed | 0 | 0 | 370 | completed | 20 | 20 | 588 |
| yourbittorrent | failed | 0 | 0 | 17 | failed | 0 | 0 | 208 |
| magnetdl | failed | 0 | 0 | 12006 | failed | 0 | 0 | 12836 |
| magnetdl_community_031 | failed | 0 | 0 | 12002 | failed | 0 | 0 | 12395 |
| solidtorrents_community_053 | failed | 0 | 0 | 1032 | completed | 0 | 0 | 2802 |
| nekobt | completed | 0 | 0 | 580 | completed | 0 | 0 | 777 |
| tsukihime | completed | 0 | 0 | 458 | completed | 0 | 0 | 535 |
| yts | completed | 0 | 0 | 989 | completed | 0 | 0 | 1336 |
| yggtracker | failed | 0 | 0 | 615 | failed | 0 | 0 | 546 |
| goggames | failed | 0 | 0 | 515 | completed | 0 | 0 | 2064 |
| snowfl | completed | 6 | 6 | 5470 | failed | 0 | 0 | 1833 |
| snowfl_community_052 | completed | 6 | 6 | 7458 | completed | 6 | 6 | 6702 |
| btdig | failed | 0 | 0 | 596 | completed | 0 | 0 | 834 |
| anidex | failed | 0 | 0 | 220 | failed | 0 | 0 | 434 |
| dmhy | failed | 0 | 0 | 290 | failed | 0 | 0 | 404 |
| dmhy_community_018 | completed | 0 | 0 | 1232 | completed | 0 | 0 | 1094 |
| mikanani | completed | 0 | 0 | 1058 | completed | 0 | 0 | 2884 |
| nyaapantsu | failed | 0 | 0 | 735 | completed | 0 | 0 | 771 |
| nyaa | failed | 0 | 0 | 114 | failed | 0 | 0 | 757 |
| torrentgalaxy | failed | 0 | 0 | 18 | failed | 0 | 0 | 316 |
| pantsu | failed | 0 | 0 | 1029 | failed | 0 | 0 | 740 |
| sukebei | failed | 0 | 0 | 110 | failed | 0 | 0 | 288 |
| uindex | failed | 0 | 0 | 275 | completed | 0 | 0 | 431 |
| uniondht | completed | 0 | 0 | 701 | completed | 0 | 0 | 2191 |
| dodi_repacks | failed | 0 | 0 | 410 | failed | 0 | 0 | 468 |
| onlinefix | failed | 0 | 0 | 297 | failed | 0 | 0 | 1665 |
| apachetorrent | completed | 0 | 0 | 1154 | completed | 0 | 0 | 1649 |
| redetorrent | completed | 0 | 0 | 1209 | completed | 0 | 0 | 943 |
| smallgames | completed | 0 | 0 | 2431 | failed | 0 | 0 | 476 |
| torrenflix | completed | 0 | 0 | 1139 | completed | 0 | 0 | 1547 |
| bitsearch | failed | 0 | 0 | 637 | timed_out | 20 | 20 | 20033 |
| therarbg | failed | 0 | 0 | 133 | timed_out | 0 | 0 | 20012 |
| torrentdownloads | failed | 9 | 9 | 2922 | timed_out | 16 | 16 | 20009 |
| traht | failed | 0 | 0 | 19 | failed | 0 | 0 | 236 |
| xxxclubto | failed | 0 | 0 | 10 | timed_out | 0 | 0 | 20016 |
| bt4gprx | failed | 0 | 0 | 494 | completed | 0 | 0 | 784 |
| linuxtracker | completed | 0 | 0 | 990 | completed | 0 | 0 | 1337 |
| sktorrent | completed | 0 | 0 | 2237 | completed | 0 | 0 | 1722 |
| tokyotoshokan | failed | 0 | 0 | 17 | failed | 0 | 0 | 205 |
| academictorrents | completed | 20 | 0 | 15220 | timed_out | 20 | 0 | 20024 |
| audiobookbay | failed | 0 | 0 | 894 | failed | 0 | 0 | 1002 |
| cpasbien | failed | 0 | 0 | 1157 | failed | 0 | 0 | 2123 |
| darklibria | failed | 0 | 0 | 840 | failed | 0 | 0 | 782 |
| dontorrent_community_020 | failed | 0 | 0 | 3635 | failed | 0 | 0 | 4888 |
| elitetorrent | completed | 0 | 0 | 768 | completed | 0 | 0 | 703 |
| eztvx | failed | 0 | 0 | 399 | completed | 0 | 0 | 586 |
| maxitorrent | failed | 0 | 0 | 949 | failed | 0 | 0 | 991 |
| mejortorrent | failed | 0 | 0 | 884 | failed | 0 | 0 | 2280 |
| pediatorrent_community_043 | failed | 0 | 0 | 300 | failed | 0 | 0 | 1098 |
| thepiratebay_community_059 | completed | 3 | 3 | 272 | completed | 3 | 3 | 495 |
| torrent9 | completed | 0 | 0 | 2016 | completed | 0 | 0 | 1602 |
| torrentdownload_community_066 | completed | 19 | 19 | 474 | completed | 20 | 20 | 584 |
| zooqle | completed | 0 | 0 | 1009 | completed | 0 | 0 | 891 |
| ali213 | completed | 0 | 0 | 9316 | failed | 0 | 0 | 13641 |
| fitgirl_repacks | completed | 1 | 0 | 4829 | completed | 1 | 0 | 6907 |
| mypornclub | failed | 0 | 0 | 109 | failed | 0 | 0 | 320 |
| rutor | completed | 0 | 0 | 570 | completed | 0 | 0 | 564 |
