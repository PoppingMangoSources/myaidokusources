<p align="center">
  <img src="media/banner.svg" alt="PoppingMango Sources — Aidoku clones of the Paperback sources" width="100%"/>
</p>

<p align="center">
  <img src="media/badge-platform.svg" alt="iOS / iPadOS" height="28"/>
  <img src="media/badge-aidoku.svg" alt="Aidoku 0.7+" height="28"/>
  <img src="media/badge-sources.svg" alt="18 sources" height="28"/>
  <img src="media/badge-unofficial.svg" alt="Best effort, not maintained 24/7" height="28"/>
</p>

<p align="center">
  <a href="https://poppingmangosources.github.io/myaidokusources/">
    <img src="media/add-button.svg" alt="Add PoppingMango to Aidoku" height="52"/>
  </a>
</p>

<p align="center">
  On iPhone or iPad, tap the button and add the source list from the page that opens.<br/>
  To add it by hand, open Aidoku, go to <b>Settings → Source Lists</b>, and paste:
</p>

<p align="center">
  <code>https://poppingmangosources.github.io/myaidokusources/index.min.json</code>
</p>

---

## Please read first

**This is a side project, not a maintained source list.**

These are Aidoku clones of my [Paperback sources](https://github.com/PoppingMangoSources/general-extensions-mangago). I built them for one reason: so I could read the sources that weren't already available on Aidoku. They're public in case they're useful to someone else too.

- **The Paperback repository is the main one.** That's where the work happens, where sources get fixed first, and where the full catalog lives.
- **These are not maintained 24/7.** When a site changes its layout or API, the Aidoku version may stay broken for a while — or indefinitely.
- **Sources already on other Aidoku repos aren't duplicated here.** If something I have on Paperback already exists in another Aidoku source list, use that one — it'll be better maintained than mine.

If you want the complete, actively maintained set, use the Paperback repository.

## Sources

**18 sources** — 14 manga, manhwa & manhua, and 4 novels.

### Manga, Manhwa & Manhua

| Source                                                                              | Site                                         |
| :---------------------------------------------------------------------------------- | :------------------------------------------- |
| <img src="media/sources/allmanga.png" width="22" align="top"/> **AllManga**         | [allmanga.to](https://allmanga.to)           |
| <img src="media/sources/bunmanga.png" width="22" align="top"/> **BunManga**         | [bunmanga.com](https://bunmanga.com)         |
| <img src="media/sources/chikari.png" width="22" align="top"/> **Chikari**           | [chikari.moe](https://chikari.moe)           |
| <img src="media/sources/galaxymanga.png" width="22" align="top"/> **Galaxy Manga**  | [galaxymanga.io](https://galaxymanga.io)     |
| <img src="media/sources/kaliscan.png" width="22" align="top"/> **KaliScan**         | [kaliscan.io](https://kaliscan.io)           |
| <img src="media/sources/kingofshojo.png" width="22" align="top"/> **KingOfShojo**   | [kingofshojo.com](https://kingofshojo.com)   |
| <img src="media/sources/likemanga.png" width="22" align="top"/> **LikeManga**       | [likemanga.ink](https://likemanga.ink)       |
| <img src="media/sources/mangatown.png" width="22" align="top"/> **MangaTown**       | [mangatown.com](https://www.mangatown.com)   |
| <img src="media/sources/omanga.png" width="22" align="top"/> **oManga**             | [omanga.to](https://omanga.to)               |
| <img src="media/sources/rinkocomics.png" width="22" align="top"/> **RinkoComics**   | [rinkocomics.com](https://rinkocomics.com)   |
| <img src="media/sources/rokaricomics.png" width="22" align="top"/> **RokariComics** | [rokaricomics.com](https://rokaricomics.com) |
| <img src="media/sources/scansgg.png" width="22" align="top"/> **Scans.GG**          | [scans.gg](https://scans.gg)                 |
| <img src="media/sources/templescan.png" width="22" align="top"/> **Temple Scan**    | [templetoons.com](https://templetoons.com)   |
| <img src="media/sources/vymanga.png" width="22" align="top"/> **VyManga**           | [vymanga.com](https://vymanga.com)           |

### Novels

| Source                                                                              | Site                                       |
| :---------------------------------------------------------------------------------- | :----------------------------------------- |
| <img src="media/sources/mvlempyr.png" width="22" align="top"/> **MVLEMPYR**         | [mvlempyr.io](https://www.mvlempyr.io)     |
| <img src="media/sources/novelarchive.png" width="22" align="top"/> **NovelArchive** | [novelarchive.cc](https://novelarchive.cc) |
| <img src="media/sources/novelcool.png" width="22" align="top"/> **NovelCool**       | [novelcool.com](https://www.novelcool.com) |
| <img src="media/sources/valirscans.png" width="22" align="top"/> **ValirScans**     | [valirscans.org](https://valirscans.org)   |

ValirScans carries both comics and novels, so it is listed once here.

## Support

<p align="center">
  <a href="https://discord.com/invite/inkdex">
    <img src="media/discord-button.svg" alt="Join the support Discord" height="36"/>
  </a>
</p>

Source problems are handled in the **OTHER-REPOS** channel of the linked Discord. Include the affected source, the title or page that failed, and a screenshot when you can.

Keep the note above in mind: Paperback fixes come first, and an Aidoku source may stay broken for a while.

## Building

Each source is a standalone Rust crate that compiles to WebAssembly.

```sh
cargo install --git https://github.com/Aidoku/aidoku-rs aidoku-cli
cd sources/en.allmanga
aidoku package
```

Pushing to `main` rebuilds every source and publishes the list to GitHub Pages.

## License

This repo is licensed under either of Apache License, version 2.0 or MIT license at your option.

These extensions are not affiliated with Aidoku, Paperback, or any supported website. All site names and logos belong to their respective owners.
