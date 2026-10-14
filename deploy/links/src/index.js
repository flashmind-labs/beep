// Share links (/s/<token>) and group invites (/g/<token>). The app reads the
// token and fetches everything over QUIC, so this side stays static: it proves
// to Android that the app owns the links, and shows a page to anyone without it.
//
// /oauth/callback is where MCP server sign-ins come back. Android opens it in
// the app as an App Link; anywhere else this page passes the query on to
// beep://oauth, and the app hands the code to the server. Nothing is kept here.

const PACKAGE = "com.flashmind.beep";
// SHA-256 of the certificates that sign the app: the upload key CI signs with,
// and the debug key of the Mac that builds the APKs installed by hand. Add the
// Play app signing key from Play Console > Test and release > App signing, or
// Play installs will not open links directly.
const FINGERPRINTS = [
  "78:DB:DE:A5:7C:78:F3:B3:FC:E3:C8:62:2E:28:BC:4A:BD:08:6B:3C:AA:A8:B3:C2:49:AD:1B:92:BE:82:55:75",
  "BA:60:80:F8:8E:A7:D2:0F:1F:10:D9:6C:1E:73:B5:90:2C:4D:34:4D:0A:4E:AC:CE:C7:BC:F3:4D:C5:34:E2:FB",
];
const PLAY = `https://play.google.com/store/apps/details?id=${PACKAGE}`;

const KINDS = {
  s: { scheme: "beep://share/", title: "A shared Beep chat", text: "Someone shared a chat with you." },
  g: { scheme: "beep://group/", title: "A Beep group invite", text: "You were invited to a group." },
};

export default {
  fetch(request) {
    const { pathname } = new URL(request.url);
    if (pathname === "/.well-known/assetlinks.json") {
      return Response.json([
        {
          relation: ["delegate_permission/common.handle_all_urls"],
          target: { namespace: "android_app", package_name: PACKAGE, sha256_cert_fingerprints: FINGERPRINTS },
        },
      ]);
    }
    if (pathname === "/oauth/callback") {
      return new Response(OAUTH_PAGE, {
        headers: {
          "content-type": "text/html; charset=utf-8",
          "cache-control": "no-store",
          "referrer-policy": "no-referrer",
          "x-robots-tag": "noindex",
        },
      });
    }
    const match = pathname.match(/^\/([sg])\/([A-Za-z0-9]+)\/?$/);
    if (!match) return new Response("Not found", { status: 404 });
    const kind = KINDS[match[1]];
    return new Response(page(kind, kind.scheme + match[2]), {
      headers: {
        "content-type": "text/html; charset=utf-8",
        "cache-control": "public, max-age=3600",
        "x-robots-tag": "noindex",
      },
    });
  },
};

function page(kind, appLink) {
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex">
<title>${kind.title}</title>
<meta property="og:title" content="${kind.title}">
<meta property="og:description" content="${kind.text} Open it in Beep.">
<style>
  :root { color-scheme: dark; }
  body { margin: 0; min-height: 100vh; display: grid; place-items: center; padding: 0 16px;
    background: #0A0E0C; color: #E8EFEA; font: 16px/1.5 system-ui, sans-serif; }
  main { max-width: 360px; width: 100%; background: #121916; border: 1px solid #243028;
    border-radius: 16px; padding: 28px 24px; text-align: center; }
  h1 { font-size: 20px; margin: 0 0 6px; }
  p { margin: 0 0 24px; color: #9DB0A5; }
  a { display: block; padding: 12px; border-radius: 10px; text-decoration: none; font-weight: 600; }
  .open { background: #3B9B7A; color: #fff; margin-bottom: 10px; }
  .get { border: 1px solid #243028; color: #E8EFEA; }
</style>
</head>
<body>
<main>
  <h1>${kind.title}</h1>
  <p>${kind.text}</p>
  <a class="open" href="${appLink}">Open in Beep</a>
  <a class="get" href="${PLAY}">Get Beep</a>
</main>
</body>
</html>`;
}

// Sends the sign-in on to the app. The query is read in the page, so nothing
// from it is written into the HTML.
const OAUTH_PAGE = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex">
<meta name="theme-color" content="#2F8A69">
<title>Back to Beep</title>
<style>
  :root {
    color-scheme: light;
    --ink: #10201A; --mint: #D5F1E3; --cream: #FBF8F1; --emerald: #2F8A69;
    --emerald-2: #3B9B7A; --coral: #F07A6A; --muted: #4E6359;
  }
  * { box-sizing: border-box; }
  body {
    margin: 0; min-height: 100vh; min-height: 100dvh; display: grid; place-items: center;
    padding: 24px 16px; color: var(--ink); background-color: var(--emerald);
    background-image: radial-gradient(rgba(16, 32, 26, .16) 1.5px, transparent 1.6px);
    background-size: 22px 22px;
    font: 16px/1.5 ui-rounded, "SF Pro Rounded", system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
    -webkit-font-smoothing: antialiased;
  }
  main {
    position: relative; width: 100%; max-width: 380px; text-align: center;
    background: var(--cream); border: 3px solid var(--ink); border-radius: 26px;
    box-shadow: 7px 7px 0 var(--ink); padding: 76px 26px 28px; margin-top: 56px;
    animation: rise .5s cubic-bezier(.2, 1.3, .4, 1) both;
  }
  .bot {
    position: absolute; left: 50%; top: 0; width: 124px; height: 124px;
    margin: -66px 0 0 -62px; padding: 10px; background: var(--emerald-2);
    border: 3px solid var(--ink); border-radius: 32px; box-shadow: 5px 5px 0 var(--ink);
  }
  .bot svg { display: block; width: 100%; height: 100%; overflow: visible; }
  .bot .head { transform-origin: 54px 72px; animation: bob 2.4s ease-in-out infinite; }
  .bot .light { animation: blink 1.2s steps(2, jump-none) infinite; }
  h1 { font-size: 26px; line-height: 1.2; letter-spacing: -.01em; margin: 0 0 8px; font-weight: 800; }
  p { margin: 0 auto 22px; max-width: 290px; color: var(--muted); }
  .btn {
    display: flex; align-items: center; justify-content: center; gap: 10px; width: 100%;
    padding: 14px 18px; border: 3px solid var(--ink); border-radius: 16px;
    background: var(--coral); color: var(--ink); font: inherit; font-weight: 800; font-size: 17px;
    text-decoration: none; box-shadow: 4px 4px 0 var(--ink);
    transition: transform .08s, box-shadow .08s;
  }
  .btn:hover { transform: translate(-1px, -1px); box-shadow: 5px 5px 0 var(--ink); }
  .btn:active { transform: translate(4px, 4px); box-shadow: 0 0 0 var(--ink); }
  .btn:focus-visible { outline: 3px solid var(--cream); outline-offset: 3px; }
  .status {
    display: inline-flex; align-items: center; gap: 8px; margin: 0 0 18px; padding: 6px 12px;
    background: var(--mint); border: 2px solid var(--ink); border-radius: 999px;
    font-size: 13px; font-weight: 700;
  }
  .dots { display: inline-flex; gap: 4px; }
  .dots i { width: 6px; height: 6px; border-radius: 50%; background: var(--ink); animation: dot 1s infinite; }
  .dots i:nth-child(2) { animation-delay: .15s; }
  .dots i:nth-child(3) { animation-delay: .3s; }
  .foot { margin: 18px 0 0; font-size: 13px; color: var(--muted); }
  .sad .wink, .sad .smile, .sad .blush { display: none; }
  .sad .eye2, .sad .flat { display: inline; }
  .eye2, .flat { display: none; }
  .sad .head, .sad .light { animation: none; }
  .sad .bot { background: #C9D3CD; }
  [hidden] { display: none !important; }
  @keyframes rise { from { opacity: 0; transform: translateY(18px) scale(.97); } }
  @keyframes bob { 50% { transform: translateY(-2.5px) rotate(-3deg); } }
  @keyframes blink { 50% { fill: #FFC2B8; } }
  @keyframes dot { 0%, 60%, 100% { transform: none; opacity: .35; } 30% { transform: translateY(-4px); opacity: 1; } }
  @media (prefers-reduced-motion: reduce) { * { animation: none !important; transition: none !important; } }
</style>
</head>
<body>
<main id="card">
  <div class="bot" aria-hidden="true">
    <svg viewBox="24 22 62 62" fill="none" stroke="#10201A" stroke-width="3" stroke-linejoin="round" stroke-linecap="round">
      <g class="head">
        <g transform="translate(3.5 3.5)" fill="#10201A">
          <path d="M29 51h5v10h-5a2 2 0 0 1-2-2v-6a2 2 0 0 1 2-2Z M74 51h5a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2h-5Z"/>
          <path d="M44 40H64A10 10 0 0 1 74 50V62A10 10 0 0 1 64 72H44A10 10 0 0 1 34 62V50A10 10 0 0 1 44 40Z"/>
          <circle cx="54" cy="30" r="4"/>
        </g>
        <path d="M54 40V34"/>
        <circle class="light" cx="54" cy="30" r="4" fill="#F07A6A"/>
        <path fill="#D5F1E3" d="M29 51h5v10h-5a2 2 0 0 1-2-2v-6a2 2 0 0 1 2-2Z M74 51h5a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2h-5Z"/>
        <path fill="#D5F1E3" d="M44 40H64A10 10 0 0 1 74 50V62A10 10 0 0 1 64 72H44A10 10 0 0 1 34 62V50A10 10 0 0 1 44 40Z"/>
        <circle cx="45" cy="54" r="4" fill="#10201A" stroke="none"/>
        <path class="wink" d="M59 55Q64 50 69 55"/>
        <circle class="eye2" cx="64" cy="54" r="4" fill="#10201A" stroke="none"/>
        <circle class="blush" cx="66.6" cy="62" r="2.6" fill="#F07A6A" stroke="none"/>
        <path class="smile" d="M47 63Q54 68.5 61 63"/>
        <path class="flat" d="M49 65H59"/>
      </g>
    </svg>
  </div>
  <div class="status" id="status"><span class="dots"><i></i><i></i><i></i></span>Opening Beep</div>
  <h1 id="title">You're signed in</h1>
  <p id="text">Beep is finishing the connection. You can head back to the app.</p>
  <a class="btn" id="open" href="beep://oauth">Open Beep</a>
  <p class="foot" id="foot">Nothing from this sign-in is kept on this page.</p>
</main>
<script>
  const query = new URLSearchParams(location.search);
  const $ = (id) => document.getElementById(id);
  if (query.get("error") || !query.get("code")) {
    document.body.classList.add("sad");
    $("status").hidden = true;
    $("title").textContent = query.get("error") ? "Sign-in cancelled" : "Nothing to finish here";
    $("text").textContent = "No server was connected. You can close this page and try again from Beep.";
    $("open").textContent = "Back to Beep";
  } else {
    const link = "beep://oauth" + location.search;
    $("open").href = link;
    location.replace(link);
    // Still here after a moment: the app did not take the link, so leave the button to it.
    setTimeout(() => {
      $("status").hidden = true;
      $("text").textContent = "Tap below to finish connecting the server in Beep.";
    }, 2500);
  }
</script>
</body>
</html>`;
