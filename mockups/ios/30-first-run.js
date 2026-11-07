// The unpaired phone opens setup before showing projects.
(() => {
  const download = 'https://releases.flashmind.com/beep/macos/Beep-latest.dmg';
  P.iConnectComputer = () => `<main class="connect-setup">
    <div class="connect-content">
      <div class="connect-mark" aria-hidden="true">${BI(32)}</div>
      <h1>Connect your computer</h1>
      <p class="connect-intro">Run coding agents, follow their progress and manage your projects from your phone.</p>
      <section class="connect-steps" aria-label="On your computer">
        <h2>On your computer</h2>
        <ol><li>Open Beep on your desktop.</li><li>Open <strong>Machines</strong>, then <strong>Add a machine</strong>.</li><li>Choose <strong>Phone</strong> to show a QR code.</li></ol>
        <p class="muted">Pairing codes work once and expire after 5 minutes.</p>
      </section>
      <a class="connect-download" href="${download}">Get Beep for Mac</a>
      ${S.pairing === 'error' ? '<p class="connect-error" role="alert">Camera unavailable. Allow camera access in Settings, or paste a pairing code.</p>' : ''}
    </div>
    <footer class="connect-actions">
      <button class="primary" data-igo="iPairingScanner">Scan QR code</button>
      <button class="text-btn" data-igo="iPairingCode">Paste pairing code</button>
      <p class="muted">Connect once. Then reach your nodes even when the desktop is offline.</p>
    </footer>
  </main>`;
  P.iPairingCode = () => iBack('Paste pairing code', 'iConnectComputer') + `<div class="body">
    <p>Copy the phone pairing code shown in Beep on your desktop.</p>
    <label for="phone-pairing-code">Pairing code</label><textarea class="field" id="phone-pairing-code" placeholder="Paste pairing code" autocapitalize="off" spellcheck="false"></textarea>
    <p class="muted">This phone gets project controls, node sessions and shared credentials. Only pair with a computer you trust.</p>
    <button class="primary" id="pairing-connect" disabled>Connect computer</button></div>`;
  P.iPairingScanner = () => iBack('Scan QR code', 'iConnectComputer') + `<div class="body">
    <p>Point your camera at the phone QR code on your desktop.</p>
    <div class="connect-camera">Camera preview</div>
    <p class="muted">This phone gets project controls, node sessions and shared credentials. Only pair with a computer you trust.</p>
    <button class="primary" data-work-go="workProjects" data-itoast="Computer connected">Simulate scan</button></div>`;
  document.addEventListener('input', event => {
    if (event.target.id !== 'phone-pairing-code') return;
    document.getElementById('pairing-connect').disabled = !event.target.value.trim();
  });
  document.addEventListener('click', event => {
    if (event.target.closest('#pairing-connect:not(:disabled)')) { go('workProjects'); toast('Computer connected'); }
  });
  VARIANTS.iConnectComputer = ['ready', 'error'].map(state => [state === 'ready' ? 'Ready to connect' : 'Camera unavailable', () => { S.pairing = state; render(); }, () => (S.pairing || 'ready') === state]);
  IOS_SCREENS.push({ group: 'First run', items: [['iConnectComputer', 'Connect your computer', 'LinkedDevicesView'], ['iPairingCode', 'Paste pairing code', 'LinkedDevicesView'], ['iPairingScanner', 'Scan QR code', 'QRScanner']] });
  render();
})();
