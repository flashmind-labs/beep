// Share links (/s/<token>) and group invites (/g/<token>). The app reads the
// token and fetches everything over QUIC, so this side stays static: it proves
// to Android that the app owns the links, and shows a page to anyone without it.

const PACKAGE = "com.flashmind.beep";
// SHA-256 of the certificates that sign the app: the upload key for sideloaded
// release builds. Add the Play app signing key from Play Console > Test and
// release > App signing, or Play installs will not open links directly.
const FINGERPRINTS = [
  "78:DB:DE:A5:7C:78:F3:B3:FC:E3:C8:62:2E:28:BC:4A:BD:08:6B:3C:AA:A8:B3:C2:49:AD:1B:92:BE:82:55:75",
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
