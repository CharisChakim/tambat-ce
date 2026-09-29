// Point each download button at the matching installer of the latest release.
// Without JavaScript, or when the GitHub API is unavailable or rate-limited,
// the buttons keep linking to the release page, which lists every installer.
(async () => {
  const patterns = {
    "windows": /-setup\.exe$/i,
    "mac-arm": /(aarch64|arm64).*\.dmg$/i,
    "mac-intel": /(x64|x86_64).*\.dmg$/i,
    "linux-appimage": /\.AppImage$/i,
    "linux-deb": /\.deb$/i,
    "linux-rpm": /\.rpm$/i,
  };
  try {
    const response = await fetch("https://api.github.com/repos/CharisChakim/tambat-releases/releases/latest");
    if (!response.ok) return;
    const release = await response.json();
    for (const link of document.querySelectorAll("[data-asset]")) {
      const asset = release.assets.find(item => patterns[link.dataset.asset].test(item.name));
      if (asset) link.href = asset.browser_download_url;
    }
    const version = document.querySelector(".download-version");
    if (version && release.tag_name) {
      version.textContent = release.tag_name;
      version.hidden = false;
    }
  } catch {
    // Network errors leave the release-page links in place.
  }
})();
