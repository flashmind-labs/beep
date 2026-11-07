// ---- Desktop: making images, videos and 3D models with the user's own keys ----
// image_gen (OpenRouter, OpenAI, Venice), video_gen (OpenRouter, Venice) and 3d_gen (Meshy) are offered
// only when a key makes them. Each asks first: the card shows the prompt, the service and the model.
// The result shows in the chat like a sent picture or file and is also saved in the home folder.
(() => {
  const st = document.createElement("style");
  st.textContent = `.mac .mg-img { width: 280px; aspect-ratio: 3 / 2; border-radius: 8px; border: 1px solid var(--d-border); cursor: zoom-in;
  background: radial-gradient(40% 50% at 70% 30%, #F0C27A 0 30%, transparent 31%), linear-gradient(180deg, #2E4E7A 0 55%, #2A2C3A 55% 100%) }
.mac .mg-saved { font-size: 11.5px; color: var(--d-muted) }
.mac .mg-saved code { font-size: 11px }`;
  document.head.append(st);
})();


// Chat: the image_gen approval card, then a made image and a 3D model in the chat.
{
  const base = D.dWinChat;
  const shield = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M12 3l8 3v6c0 4.5-3.4 8.2-8 9-4.6-.8-8-4.5-8-9V6z"/></svg>';
  const cube = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M12 2.5l8.5 4.7v9.6L12 21.5l-8.5-4.7V7.2z"/><path d="M3.5 7.2L12 12l8.5-4.8M12 12v9.5"/></svg>';
  const ask = "Make a poster of a lighthouse at dawn, wide, then a 3D model of the lighthouse";
  const card = `<div class="dcard">
      <div class="head"><span class="dapp">M</span><div style="flex:1;min-width:0;display:grid"><b style="font-size:13.5px">Make an image with OpenAI?</b><span style="font-size:12px;color:var(--d-muted)">Runs on your OpenAI key and costs you money</span></div></div>
      <div class="kv"><span>Prompt</span><span>A poster of a lighthouse at dawn, warm light, calm sea</span><span>Provider</span><span>OpenAI</span><span>Model</span><span>gpt-image-2</span><span>Aspect ratio</span><span>16:9</span></div>
      <div class="acts"><button class="dbtn pri" data-toast="Allowed once">Allow<kbd>⌘${DI.ret}</kbd></button><button class="dbtn" data-toast="Always allowed for Media generation">Always<kbd>⌥⌘${DI.ret}</kbd></button><button class="dbtn warn" data-toast="Denied">Deny<kbd>⌘D</kbd></button></div>
    </div>`;
  const asking = `${dHead("Working for 4s", { stats: "1 tool call" })}<div class="tools">${dTool("Making an image", "run")}</div>${dLive("Working", "4s")}<div class="dcoderen agent md">Here is what I will ask OpenAI for. It runs once you allow it.</div>${card}`;
  const made = `${dFold(["Made an image", "Made a 3D model"], 38)}
    <div class="dmuted">${shield}Allowed once</div>
    <div class="dcoderen agent md">Here is the poster, made with OpenAI's gpt-image-2.</div>
    <div class="mg-img" role="img" aria-label="A lighthouse at dawn" data-toast="Opens the picture viewer"></div>
    <div class="mg-saved">Saved as <code>lighthouse-at-dawn-warm-light-a3f9c1.png</code> in your home folder</div>
    <div class="dcoderen agent md">And the lighthouse as a textured 3D model from Meshy:</div>
    <button class="dfile" data-toast="Opens in Preview"><span class="ic">${cube}</span>
      <span style="flex:1;min-width:0;display:grid"><span class="nm">lighthouse-7be20d.glb</span><span class="sz">3D model, 4.2 MB</span></span><span style="color:var(--d-muted);display:grid">${DI.chev}</span></button>
    <div class="mg-saved">Saved as <code>lighthouse-7be20d.glb</code> in your home folder</div>`;
  const making = `${dHead("Working for 22s", { stats: "2 tool calls" })}<div class="tools">${dTool("Made an image")}${dTool("Making a 3D model", "run")}</div>${dLive("Working", "4s")}
    <div class="dmuted"><span class="dspin"></span>Meshy is painting the textures</div>`;
  D.dWinChat = () => {
    const v = S.d.wc;
    if (!["mg-ask", "mg-run", "mg-done"].includes(v)) return base();
    const keep = S.d.wc; S.d.wc = "reply";
    const html = base();
    S.d.wc = keep;
    const body = { "mg-ask": asking, "mg-run": making, "mg-done": made }[v];
    // The chat's turns end with "</div></div>" right before the composer.
    const at = html.indexOf('<div class="cc-comp">');
    const end = html.lastIndexOf("</div></div>", at);
    return html.slice(0, end) + ccTurn(ask, body) + html.slice(end);
  };
  VARIANTS.dWinChat.push(...[["Make an image, asking", "mg-ask"], ["Making a 3D model", "mg-run"], ["Made an image and a 3D model", "mg-done"]]
    .map(([l, k]) => [l, () => { S.d.wc = k; render(); }, () => S.d.wc === k]));
}
