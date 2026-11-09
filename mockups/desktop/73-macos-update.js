// Sparkle owns the standard native macOS update dialog; this is a content preview.
(() => {
  D.dMacUpdate = () => deskScene(`<section class="sparkle-preview" role="dialog" aria-labelledby="update-title">
    <h2 id="update-title">A new version of Beep is available!</h2>
    <p>Beep 0.7.1 is now available. You have 0.7.0.</p>
    <p>Would you like to download it now?</p>
    <div class="sparkle-notes"><h3>Release notes</h3><h4>Added</h4><ul>
      <li>macOS updates show release notes in Sparkle's standard native dialog.</li>
      <li>A versioned changelog records changes in each release.</li>
    </ul></div>
    <div class="sparkle-actions"><button data-toast="This version will be skipped" data-dgo="dHome">Skip this version</button>
      <button data-dgo="dHome">Remind me later</button><button class="primary" data-toast="Downloading update">Install update</button></div>
  </section>`);
  DESK_SCREENS.push({ group: 'App updates', items: [['dMacUpdate', 'macOS update', 'Standard Sparkle release notes and installation']] });
})();
