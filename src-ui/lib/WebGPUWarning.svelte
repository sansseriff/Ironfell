<script lang="ts">
  interface Props {
    show: boolean;
    onDismiss: () => void;
  }

  let { show, onDismiss }: Props = $props();

  // Browser detection
  function detectBrowser() {
    const userAgent = navigator.userAgent.toLowerCase();
    const isChrome = userAgent.includes("chrome") && !userAgent.includes("edg");
    const isEdge = userAgent.includes("edg");
    const isFirefox = userAgent.includes("firefox");
    const isSafari =
      userAgent.includes("safari") && !userAgent.includes("chrome");
    const isBrave = !!(navigator as any).brave;

    if (isBrave) return "brave";
    if (isChrome) return "chrome";
    if (isEdge) return "edge";
    if (isFirefox) return "firefox";
    if (isSafari) return "safari";
    return "unknown";
  }

  function isMobile() {
    return /android|webos|iphone|ipad|ipod|blackberry|iemobile|opera mini/i.test(
      navigator.userAgent
    );
  }

  const browser = detectBrowser();
  const mobile = isMobile();
  let showTechnicalDetails = $state(false);

  function toggleTechnicalDetails() {
    showTechnicalDetails = !showTechnicalDetails;
  }

  // Browser-specific messages
  const browserMessages = {
    chrome: {
      title: "WebGPU Not Enabled in Chrome",
      description:
        "Chrome supports WebGPU, but it might not be enabled on your system.",
      instructions: [
        "Update to the latest version of Chrome (version 113 or later)",
        "WebGPU should be enabled by default in recent versions",
        "If still not working, go to chrome://flags and search for 'WebGPU'",
        "Enable 'Unsafe WebGPU' if available",
      ],
      technicalSolutions: [
        "Check if your GPU drivers are up to date",
        "Ensure hardware acceleration is enabled in Chrome settings",
        "Try running Chrome with --enable-unsafe-webgpu flag",
        "Verify your GPU is not on Chrome's blocklist",
      ],
    },
    edge: {
      title: "WebGPU Not Enabled in Edge",
      description:
        "Microsoft Edge supports WebGPU, but it might not be enabled.",
      instructions: [
        "Update to the latest version of Edge",
        "WebGPU should be enabled by default in recent versions",
        "If still not working, go to edge://flags and search for 'WebGPU'",
        "Enable 'Unsafe WebGPU' if available",
      ],
      technicalSolutions: [
        "Update your graphics drivers",
        "Check edge://gpu for GPU status and issues",
        "Enable hardware acceleration in Edge settings",
        "Try launching Edge with --enable-unsafe-webgpu",
      ],
    },
    brave: {
      title: "WebGPU Not Enabled in Brave",
      description:
        "Brave Browser supports WebGPU, but it might not be enabled.",
      instructions: [
        "Update to the latest version of Brave",
        "Go to brave://flags and search for 'WebGPU'",
        "Enable 'Unsafe WebGPU'",
        "Restart the browser after enabling",
      ],
      technicalSolutions: [
        "Disable Brave's aggressive privacy features temporarily",
        "Check if Brave Shield is blocking WebGPU",
        "Update GPU drivers and restart",
        "Try launching with --enable-unsafe-webgpu flag",
      ],
    },
    firefox: {
      title: "WebGPU Not Enabled in Firefox",
      description: "WebGPU is not enabled in Firefox by default.",
      instructions: [
        "Try Firefox Nightly, though expect instability as of July 2025",
      ],
      technicalSolutions: [
        "Check about:support for GPU information",
        "Try Firefox Nightly for latest WebGPU features",
      ],
    },
    safari: {
      title: "WebGPU Not Available in Safari",
      description:
        "Safari does not support WebGPU in stable releases. WebGPU is only available in Safari Technology Preview.",
      instructions: [
        "Download Safari Technology Preview from Apple's developer site",
        "Install and use Safari Technology Preview instead",
        "Alternatively, use Chrome, Edge, or Brave for WebGPU support",
      ],
      technicalSolutions: [
        "Safari Technology Preview requires macOS 13.3 or later",
        "Enable Develop menu in Safari TP: Safari > Preferences > Advanced",
        "WebGPU support is experimental even in Technology Preview",
        "Consider using Chrome/Edge for production WebGPU apps",
      ],
    },
    unknown: {
      title: "WebGPU Not Supported",
      description:
        "Your browser doesn't appear to support WebGPU or it's not enabled.",
      instructions: [
        "Try using Chrome, Edge, or Brave (latest versions)",
        "Update your browser to the latest version",
        "Check if WebGPU flags are enabled in your browser settings",
      ],
      technicalSolutions: [
        "Verify your browser version supports WebGPU",
        "Check if your GPU hardware supports WebGPU",
        "Update your graphics drivers",
        "Try different browsers to isolate the issue",
      ],
    },
  };

  const currentMessage = browserMessages[browser];
</script>

{#if show}
  <div id="webgpu-warning">
    <div class="warning-content">
      <div class="warning-text">
        <h3>{currentMessage.title}</h3>
        <p>{currentMessage.description}</p>

        {#if mobile}
          <div class="warning-details mobile-warning">
            <p><strong>📱 Mobile Device Detected</strong></p>
            <p>
              WebGPU support on mobile devices is very limited. This app works
              best on desktop browsers.
            </p>
            <p>
              If you're on a desktop, try using Chrome, Edge, or Brave instead.
            </p>
          </div>
        {:else}
          <div class="warning-details">
            <p><strong>How to fix this:</strong></p>
            <ol>
              {#each currentMessage.instructions as instruction}
                <li>{instruction}</li>
              {/each}
            </ol>

            <button class="technical-toggle" onclick={toggleTechnicalDetails}>
              {showTechnicalDetails ? "▼" : "▶"} More Information
            </button>

            {#if showTechnicalDetails}
              <div class="technical-details">
                <p><strong>Advanced troubleshooting:</strong></p>
                <ul>
                  {#each currentMessage.technicalSolutions as solution}
                    <li>{solution}</li>
                  {/each}
                </ul>

                <div class="implementation-status">
                  <p><strong>📋 Browser Implementation Status:</strong></p>
                  <p>
                    For detailed information about WebGPU support across
                    browsers, see the
                    <a
                      href="https://github.com/gpuweb/gpuweb/wiki/Implementation-Status"
                      target="_blank"
                      rel="noopener noreferrer"
                    >
                      official WebGPU Implementation Status
                    </a>
                  </p>
                </div>
              </div>
            {/if}
          </div>
        {/if}

        <div class="browser-info">
          <p>
            <strong>Detected:</strong>
            {browser.charAt(0).toUpperCase() + browser.slice(1)}
            {mobile ? "(Mobile)" : "(Desktop)"}
          </p>
        </div>

        <button class="warning-dismiss" onclick={onDismiss}>Dismiss</button>
      </div>
    </div>
  </div>
{/if}

<style>
  #webgpu-warning {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    background-color: rgba(0, 0, 0, 0.95);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
    padding: 20px;
    box-sizing: border-box;
    background-color: white;
  }

  .warning-content {
    background: linear-gradient(135deg, #ffd2d2, #ffe8be);
    color: black;
    border-radius: 16px;
    padding: 30px;
    max-width: 600px;
    width: 100%;
    box-shadow: 0 20px 40px rgba(0, 0, 0, 0.1);
    animation: warningSlideIn 0.5s ease-out;
    font-family:
      -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  }

  .warning-icon {
    font-size: 48px;
    text-align: center;
    margin-bottom: 20px;
    animation: warningPulse 2s infinite;
  }

  .warning-text h3 {
    font-size: 24px;
    margin: 0 0 15px 0;
    text-align: center;
    font-weight: 600;
  }

  .warning-text p {
    font-size: 16px;
    line-height: 1.6;
    margin: 0 0 20px 0;
    text-align: center;
  }

  .warning-details {
    background: rgba(255, 255, 255, 0.1);
    border-radius: 8px;
    padding: 20px;
    margin: 20px 0;
    backdrop-filter: blur(10px);
  }

  .warning-details p {
    margin: 0 0 15px 0;
    text-align: left;
  }

  .warning-details ol {
    margin: 10px 0;
    padding-left: 20px;
  }

  .warning-details li {
    margin: 8px 0;
    line-height: 1.4;
  }

  .technical-toggle {
    background: rgba(255, 255, 255, 0.1);
    border: 1px solid rgba(126, 126, 126, 0.2);
    color: rgb(46, 46, 46);
    padding: 8px 12px;
    border-radius: 6px;
    cursor: pointer;
    font-size: 14px;
    margin: 15px 0 0 0;
    transition: all 0.3s ease;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .technical-toggle:hover {
    background: rgba(255, 255, 255, 0.2);
    border-color: rgba(72, 72, 72, 0.3);
  }

  .technical-details {
    margin-top: 15px;
    padding: 15px;
    background: rgba(255, 255, 255, 0.05);
    border-radius: 6px;
    border: 1px solid rgba(55, 55, 55, 0.1);
    animation: slideDown 0.3s ease-out;
  }

  .technical-details ul {
    margin: 10px 0;
    padding-left: 20px;
  }

  .technical-details li {
    margin: 6px 0;
    line-height: 1.3;
    font-size: 14px;
    opacity: 0.9;
  }

  .implementation-status {
    margin-top: 15px;
    padding: 12px;
    background: rgba(100, 200, 255, 0.1);
    border-radius: 6px;
    border: 1px solid rgba(54, 54, 54, 0.2);
  }

  .implementation-status p {
    margin: 0 0 8px 0;
    font-size: 14px;
    color: rgb(60, 60, 60);
  }

  .implementation-status a {
    color: #87ceeb;
    text-decoration: underline;
  }

  .implementation-status a:hover {
    color: #b0e0e6;
  }

  @keyframes slideDown {
    from {
      opacity: 0;
      transform: translateY(-10px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .mobile-warning {
    background: rgba(255, 193, 7, 0.2);
    border: 1px solid rgba(255, 193, 7, 0.3);
  }

  .browser-info {
    background: rgba(255, 255, 255, 0.05);
    border-radius: 6px;
    padding: 10px;
    margin: 15px 0;
    text-align: center;
    font-size: 14px;
    opacity: 0.8;
  }

  .browser-info p {
    margin: 0;
  }

  .warning-dismiss {
    display: block;
    margin: 20px auto 0;
    padding: 12px 24px;
    background: rgba(255, 255, 255, 0.2);
    border: 2px solid rgba(95, 95, 95, 0.3);
    border-radius: 8px;
    color: rgb(44, 44, 44);
    font-size: 16px;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.3s ease;
    backdrop-filter: blur(10px);
  }

  .warning-dismiss:hover {
    background: rgba(255, 255, 255, 0.3);
    border-color: rgba(204, 204, 204, 0.5);
    transform: translateY(-2px);
  }

  .warning-dismiss:active {
    transform: translateY(0);
  }

  @keyframes warningSlideIn {
    from {
      opacity: 0;
      transform: translateY(-50px) scale(0.9);
    }
    to {
      opacity: 1;
      transform: translateY(0) scale(1);
    }
  }

  @keyframes warningPulse {
    0%,
    100% {
      transform: scale(1);
    }
    50% {
      transform: scale(1.1);
    }
  }

  /* Mobile responsiveness */
  @media (max-width: 768px) {
    #webgpu-warning {
      padding: 15px;
    }

    .warning-content {
      padding: 20px;
      border-radius: 12px;
    }

    .warning-icon {
      font-size: 36px;
      margin-bottom: 15px;
    }

    .warning-text h3 {
      font-size: 20px;
    }

    .warning-text p {
      font-size: 14px;
    }

    .warning-details {
      padding: 15px;
    }

    .warning-details li {
      font-size: 14px;
    }

    .technical-toggle {
      font-size: 13px;
      padding: 6px 10px;
    }

    .technical-details {
      padding: 12px;
    }

    .technical-details li {
      font-size: 13px;
    }

    .implementation-status {
      padding: 10px;
    }

    .implementation-status p {
      font-size: 13px;
    }
  }

  @media (max-width: 480px) {
    .warning-content {
      padding: 15px;
      border-radius: 8px;
    }

    .warning-icon {
      font-size: 32px;
      margin-bottom: 10px;
    }

    .warning-text h3 {
      font-size: 18px;
    }

    .warning-text p {
      font-size: 13px;
    }

    .warning-details {
      padding: 12px;
    }

    .warning-details li {
      font-size: 13px;
    }

    .technical-toggle {
      font-size: 12px;
      padding: 5px 8px;
    }

    .technical-details {
      padding: 10px;
    }

    .technical-details li {
      font-size: 12px;
    }

    .implementation-status {
      padding: 8px;
    }

    .implementation-status p {
      font-size: 12px;
    }

    .warning-dismiss {
      padding: 10px 20px;
      font-size: 14px;
    }
  }
</style>
