chrome.runtime.onMessage.addListener((request, sender, sendResponse) => {
  if (request.action === "getPageInfo") {
    const video = document.querySelector('video');
    const currentTime = video ? video.currentTime : 0;
    const duration = video ? video.duration : 0;
    
    // Try to isolate YouTube's specific title element, fallback to document title
    let title = document.title;
    const ytTitleEl = document.querySelector('ytd-watch-metadata h1, h1.title.style-scope.ytd-video-primary-info-renderer');
    if (ytTitleEl) {
      title = ytTitleEl.textContent.trim();
    }
    
    // Grab currently selected text to automatically fill context
    const selectedText = window.getSelection().toString().trim();
    
    sendResponse({
      title: title,
      url: window.location.href,
      currentTime: currentTime,
      duration: duration,
      selectedText: selectedText
    });
  }
  return true;
});
