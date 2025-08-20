<a
  class="gh-btn"
  href="https://github.com/sansseriff/Ironfell"
  target="_blank"
  rel="noopener noreferrer"
  aria-label="Open Ironfell repository on GitHub"
>
  <svg class="icon" viewBox="0 0 16 16" aria-hidden="true">
    <path
      d="M8 .2a8 8 0 0 0-2.53 15.59c.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2 .37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.01.08-2.11 0 0 .67-.21 2.2.82a7.63 7.63 0 0 1 2-.27c.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.91.08 2.11.51.56.82 1.28.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.24.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8 8 0 0 0 8 .2Z"
      fill="currentColor"
    />
  </svg>
  <span>View on GitHub</span>
</a>

<style>
  .gh-btn {
    --bg-start: hsl(221, 8%, 55%);
    --bg-end: hsl(220, 13%, 73%);
    --border: hsla(222, 12%, 71%, 0.6);
    --text: hsl(0 0% 98%);
    --accent: hsl(221, 14%, 74%);
    --glow: hsla(220, 12%, 69%, 0.4);

    display: inline-flex;
    align-items: center;
    gap: 0.55rem;
    font:
      500 0.9rem/1 system-ui,
      -apple-system,
      Segoe UI,
      Roboto,
      sans-serif;
    padding: 0.65rem 1.1rem 0.6rem;
    color: var(--text);
    background: linear-gradient(145deg, var(--bg-start), var(--bg-end));
    border: 1px solid var(--border);
    border-radius: 0.3rem;
    text-decoration: none;
    position: relative;
    transition:
      background 0.35s,
      transform 0.2s,
      border-color 0.3s,
      box-shadow 0.3s;
    box-shadow:
      0 2px 4px -2px hsl(0 0% 0% / 45%),
      0 4px 18px -6px hsl(220 60% 20% / 35%);
  }

  .gh-btn .icon {
    width: 1.05rem;
    height: 1.05rem;
  }

  .gh-btn:hover,
  .gh-btn:focus-visible {
    background: linear-gradient(145deg, hsl(222 18% 22%), hsl(222 20% 14%));
    border-color: hsl(220 60% 50% / 70%);
    box-shadow: 0 3px 6px -2px hsl(0 0% 0% / 55%),
      0 0 0 10pxrgba(208, 208, 208, 0.2)),
      0 5px 22px -6px var(--glow);
  }

  .gh-btn:active {
    transform: translateY(1px);
    box-shadow:
      0 2px 4px -2px hsl(0 0% 0% / 55%),
      0 0 0 3px hsl(220 100% 60% / 15%),
      0 3px 14px -4px var(--glow);
  }

  @media (prefers-color-scheme: light) {
    .gh-btn {
      --bg-start: hsl(220 50% 98%);
      --bg-end: hsl(220 40% 94%);
      --border: hsl(220 30% 80%);
      --text: hsl(220 25% 20%);
      --accent: hsl(220 65% 54%);
      --glow: hsl(220 85% 60% / 45%);
      box-shadow:
        0 2px 4px -2px hsl(220 40% 30% / 25%),
        0 4px 16px -8px hsl(220 55% 40% / 20%);
    }
    .gh-btn:hover,
    .gh-btn:focus-visible {
      background: linear-gradient(145deg, hsl(220 55% 100%), hsl(220 45% 96%));
      border-color: hsl(220 65% 60%);
    }
  }

  .gh-btn:focus-visible {
    outline: 0;
  }
</style>
