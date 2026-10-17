# Live search of all public plugins

Query: `nirvana in fire`. Category: all. Per-source timeout: 20s, limit: 20, parallel: 6.

Rust and Python ran sequentially against live websites. Network/cache/site changes affect comparisons; these are observations, not a controlled speed benchmark. Jackett has no local credentials. Completed with zero results does not prove that a site parser works.

Title matches are advisory checks for Nirvana in Fire / Lang Ya Bang / 琅琊榜, not full content validation. No torrent payload was downloaded.

| Source | Rust status | Results | Title matches | ms | Python status | Results | Title matches | ms |
|---|---|---:|---:|---:|---|---:|---:|---:|
| eztv | failed | 0 | 0 | 460 | failed | 0 | 0 | 675 |
| jackett | failed | 0 | 0 | 0 | completed | 1 | 1 | 442 |
| limetorrents | completed | 20 | 19 | 924 | completed | 20 | 19 | 1454 |
| piratebay | completed | 3 | 3 | 629 | completed | 3 | 3 | 586 |
| solidtorrents | failed | 0 | 0 | 3039 | failed | 0 | 0 | 16199 |
| torlock | failed | 0 | 0 | 475 | failed | 0 | 0 | 787 |
| torrentproject | failed | 0 | 0 | 6644 | failed | 0 | 0 | 14581 |
| torrentscsv | completed | 0 | 0 | 2656 | completed | 0 | 0 | 2873 |
| acgrip | completed | 0 | 0 | 3157 | completed | 0 | 0 | 3115 |
| animetosho | failed | 0 | 0 | 75 | failed | 0 | 0 | 352 |
| animetosho_community_006 | completed | 2 | 0 | 785 | completed | 2 | 0 | 956 |
| cloudtorrents | failed | 0 | 0 | 514 | failed | 0 | 0 | 745 |
| mikan | completed | 0 | 0 | 616 | completed | 0 | 0 | 1040 |
| nyaasi | completed | 0 | 0 | 2721 | completed | 0 | 0 | 3007 |
| subsplease | completed | 0 | 0 | 415 | completed | 0 | 0 | 509 |
| sukebeisi | completed | 0 | 0 | 2773 | completed | 0 | 0 | 2656 |
| thepiratebay | completed | 3 | 3 | 238 | completed | 3 | 3 | 488 |
| torrentclaw | completed | 0 | 0 | 810 | completed | 0 | 0 | 517 |
| divxtotal | failed | 0 | 0 | 5098 | failed | 0 | 0 | 12361 |
| dontorrent | failed | 0 | 0 | 1953 | failed | 0 | 0 | 12368 |
| esmeraldatorrent | failed | 0 | 0 | 1908 | failed | 0 | 0 | 3370 |
| naranjatorrent | failed | 0 | 0 | 8031 | failed | 0 | 0 | 5982 |
| tomadivx | failed | 0 | 0 | 1615 | failed | 0 | 0 | 5055 |
| calidadtorrent | failed | 0 | 0 | 411 | failed | 0 | 0 | 439 |
| pediatorrent | failed | 0 | 0 | 439 | failed | 0 | 0 | 464 |
| glotorrents | failed | 0 | 0 | 6763 | failed | 0 | 0 | 15211 |
| kickasstorrents | failed | 0 | 0 | 447 | failed | 0 | 0 | 488 |
| pirateiro | completed | 0 | 0 | 2835 | completed | 0 | 0 | 2681 |
| rockbox | failed | 0 | 0 | 8014 | failed | 0 | 0 | 12293 |
| torrentdownload | completed | 0 | 0 | 614 | completed | 20 | 20 | 662 |
| yourbittorrent | failed | 0 | 0 | 395 | failed | 0 | 0 | 451 |
| magnetdl | failed | 0 | 0 | 12008 | failed | 0 | 0 | 12569 |
| magnetdl_community_031 | failed | 0 | 0 | 12006 | failed | 0 | 0 | 12358 |
| solidtorrents_community_053 | failed | 0 | 0 | 5425 | failed | 0 | 0 | 15190 |
| nekobt | completed | 0 | 0 | 843 | completed | 0 | 0 | 887 |
| tsukihime | completed | 0 | 0 | 522 | completed | 0 | 0 | 481 |
| yts | completed | 0 | 0 | 617 | completed | 0 | 0 | 776 |
| yggtracker | failed | 0 | 0 | 8571 | failed | 0 | 0 | 12709 |
| goggames | failed | 0 | 0 | 419 | completed | 0 | 0 | 495 |
| snowfl | completed | 6 | 6 | 5649 | failed | 0 | 0 | 1338 |
| snowfl_community_052 | failed | 0 | 0 | 557 | completed | 6 | 6 | 6218 |
| btdig | failed | 0 | 0 | 8006 | timed_out | 0 | 0 | 20018 |
| anidex | failed | 0 | 0 | 3218 | failed | 0 | 0 | 2578 |
| dmhy | completed | 0 | 0 | 869 | completed | 0 | 0 | 986 |
| dmhy_community_018 | completed | 0 | 0 | 1451 | completed | 0 | 0 | 1545 |
| mikanani | completed | 0 | 0 | 2259 | completed | 0 | 0 | 1182 |
| nyaapantsu | failed | 0 | 0 | 8012 | completed | 0 | 0 | 12373 |
| nyaa | completed | 0 | 0 | 3941 | failed | 0 | 0 | 3079 |
| torrentgalaxy | failed | 0 | 0 | 84 | failed | 0 | 0 | 431 |
| pantsu | failed | 0 | 0 | 550 | failed | 0 | 0 | 532 |
| sukebei | failed | 0 | 0 | 511 | failed | 0 | 0 | 503 |
| uindex | failed | 0 | 0 | 354 | completed | 0 | 0 | 562 |
| uniondht | failed | 0 | 0 | 8003 | completed | 0 | 0 | 2834 |
| dodi_repacks | failed | 0 | 0 | 379 | failed | 0 | 0 | 489 |
| onlinefix | failed | 0 | 0 | 312 | failed | 0 | 0 | 427 |
| apachetorrent | completed | 0 | 0 | 1288 | completed | 0 | 0 | 1224 |
| redetorrent | completed | 0 | 0 | 1107 | completed | 0 | 0 | 1896 |
| smallgames | completed | 0 | 0 | 981 | failed | 0 | 0 | 1639 |
| torrenflix | completed | 0 | 0 | 3851 | completed | 0 | 0 | 3763 |
| bitsearch | failed | 0 | 0 | 351 | failed | 20 | 20 | 15202 |
| therarbg | completed | 20 | 3 | 9091 | completed | 0 | 0 | 2363 |
| torrentdownloads | failed | 9 | 9 | 5281 | completed | 20 | 20 | 16416 |
| traht | failed | 0 | 0 | 499 | failed | 0 | 0 | 842 |
| xxxclubto | completed | 0 | 0 | 409 | completed | 0 | 0 | 899 |
| bt4gprx | failed | 0 | 0 | 497 | completed | 0 | 0 | 985 |
| linuxtracker | completed | 0 | 0 | 940 | completed | 0 | 0 | 1485 |
| sktorrent | completed | 0 | 0 | 5104 | completed | 0 | 0 | 5685 |
| tokyotoshokan | failed | 0 | 0 | 538 | failed | 0 | 0 | 883 |
| academictorrents | completed | 20 | 0 | 2643 | timed_out | 20 | 0 | 20030 |
| audiobookbay | failed | 0 | 0 | 2812 | failed | 0 | 0 | 4795 |
| cpasbien | failed | 0 | 0 | 901 | failed | 0 | 0 | 1191 |
| darklibria | failed | 0 | 0 | 8011 | failed | 0 | 0 | 12726 |
| dontorrent_community_020 | failed | 0 | 0 | 12006 | failed | 0 | 0 | 6151 |
| elitetorrent | completed | 0 | 0 | 822 | completed | 0 | 0 | 1333 |
| eztvx | timed_out | 0 | 0 | 20005 | timed_out | 0 | 0 | 20029 |
| maxitorrent | failed | 0 | 0 | 3133 | failed | 0 | 0 | 3080 |
| mejortorrent | failed | 0 | 0 | 374 | failed | 0 | 0 | 705 |
| pediatorrent_community_043 | failed | 0 | 0 | 249 | failed | 0 | 0 | 729 |
| thepiratebay_community_059 | completed | 3 | 3 | 410 | completed | 3 | 3 | 820 |
| torrent9 | failed | 0 | 0 | 6510 | completed | 0 | 0 | 7593 |
| torrentdownload_community_066 | completed | 19 | 19 | 435 | completed | 20 | 20 | 892 |
| zooqle | completed | 0 | 0 | 2695 | completed | 0 | 0 | 4250 |
| ali213 | completed | 0 | 0 | 12363 | timed_out | 0 | 0 | 20031 |
| fitgirl_repacks | completed | 1 | 0 | 4296 | completed | 1 | 0 | 8786 |
| mypornclub | completed | 0 | 0 | 419 | completed | 0 | 0 | 887 |
| rutor | completed | 0 | 0 | 2568 | completed | 0 | 0 | 3982 |
