<script>
  export let navHtml = '';
  export let accHtml = '';
  export let accountActive = false;
</script>

<aside>
  <div class="logo"><svg width="34" height="34" viewBox="0 0 34 34" aria-hidden="true"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#C4B5FD"/><stop offset=".5" stop-color="#8B5CF6"/><stop offset="1" stop-color="#C026D3"/></linearGradient></defs><polygon points="17,2 30,12 24,32 10,32 4,12" fill="url(#g)"/><polygon points="17,2 30,12 17,18" fill="rgba(255,255,255,.35)"/></svg>Obsidia</div>
  <nav id="nav">{@html navHtml}</nav>
  <button class="acc gl" class:on={accountActive} id="acc" data-act="go" data-t="account" aria-label="Abrir mi cuenta">{@html accHtml}</button>
</aside>
