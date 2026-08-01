use crate::models::*;
use aidoku::{
	Result,
	alloc::{String, Vec},
	imports::js::WebView,
	imports::net::Request,
	imports::std::sleep,
	prelude::*,
};

/// Captures the reader's decoded page payload.
///
/// The reader fetches its pages client side, so `JSON.parse` is proxied before
/// the page's own scripts run and the matching object is stashed on `window`.
const CAPTURE_SCRIPT: &str = r#"<script>
(function () {
  window.__amPages__ = "";
  function capture(parsed, raw) {
    try {
      if (parsed && (parsed.chapterPages || (parsed.data && parsed.data.chapterPages))) {
        if (!window.__amPages__) window.__amPages__ = raw || JSON.stringify(parsed);
      }
    } catch (e) {}
  }
  var origParse = JSON.parse;
  JSON.parse = function (text) {
    var parsed = origParse.apply(this, arguments);
    capture(parsed, typeof text === "string" ? text : null);
    return parsed;
  };
  if (typeof Response !== "undefined" && Response.prototype && Response.prototype.json) {
    var origJson = Response.prototype.json;
    Response.prototype.json = function () {
      return origJson.call(this).then(function (parsed) {
        capture(parsed, null);
        return parsed;
      });
    };
  }
})();
</script>"#;

/// Reads page urls straight off the rendered reader as a last resort.
const SCRAPE_SCRIPT: &str = r#"(function () {
  var out = [];
  var seen = {};
  var images = document.querySelectorAll("img");
  for (var i = 0; i < images.length; i++) {
    var src = images[i].currentSrc || images[i].getAttribute("src") || "";
    if (!src || src.indexOf("data:") === 0) continue;
    if (src.indexOf("youtube-anime") === -1 && src.indexOf("/manga/") === -1) continue;
    if (seen[src]) continue;
    seen[src] = 1;
    out.push(src);
  }
  return JSON.stringify(out);
})()"#;

fn inject(html: &str) -> String {
	match html.find("<head>") {
		Some(index) => {
			let (start, rest) = html.split_at(index + "<head>".len());
			format!("{start}{CAPTURE_SCRIPT}{rest}")
		}
		None => format!("{CAPTURE_SCRIPT}{html}"),
	}
}

/// Loads a chapter in a background web view and returns its page urls.
pub fn page_urls_via_webview(manga_id: &str, chapter: &str) -> Result<Vec<String>> {
	let reader_url = format!("{DOMAIN}/manga/{manga_id}/chapter-{chapter}-sub");
	let html = Request::get(&reader_url)?
		.header("Referer", &format!("{DOMAIN}/"))
		.send()?
		.get_string()?;

	let webview = WebView::new();
	webview
		.load_html_blocking(&inject(&html), Some(&reader_url))
		.map_err(|_| error!("Unable to load the reader"))?;

	// The payload lands once the reader's own fetch resolves, so give it a moment.
	for attempt in 0..10 {
		if attempt > 0 {
			sleep(1);
		}
		if let Ok(captured) = webview.eval("window.__amPages__ || \"\"")
			&& !captured.is_empty()
			&& let Some(urls) = urls_from_payload(&captured)
			&& !urls.is_empty()
		{
			return Ok(urls);
		}
	}

	let scraped = webview
		.eval(SCRAPE_SCRIPT)
		.map_err(|_| error!("Unable to read the reader pages"))?;
	Ok(serde_json::from_str::<Vec<String>>(&scraped).unwrap_or_default())
}

/// Pulls page urls out of a captured `chapterPages` payload.
fn urls_from_payload(raw: &str) -> Option<Vec<String>> {
	#[derive(serde::Deserialize)]
	struct Envelope {
		#[serde(rename = "chapterPages")]
		chapter_pages: Option<ChapterPages>,
		data: Option<Inner>,
	}

	#[derive(serde::Deserialize)]
	struct Inner {
		#[serde(rename = "chapterPages")]
		chapter_pages: Option<ChapterPages>,
	}

	let envelope: Envelope = serde_json::from_str(raw).ok()?;
	let pages = envelope
		.chapter_pages
		.or_else(|| envelope.data.and_then(|inner| inner.chapter_pages))?;
	Some(crate::parsers::parse_page_urls(
		&pages,
		&crate::settings::image_quality(),
	))
}
