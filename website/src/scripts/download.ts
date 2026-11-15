import { resolveDownload, releasesPage } from '../lib/releases';

const button = document.querySelector<HTMLAnchorElement>('[data-download]');
if (button) {
  void resolveDownload().then((url) => {
    if (url === releasesPage) return;
    button.href = url;
    // Let GitHub's attachment response start the download in the current tab.
    button.removeAttribute('target');
  });
}
