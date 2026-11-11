<script lang="ts">
  // Pantalla superpuesta del editor de skins (Etapa 1, aislada).
  // Vive en el template Svelte de +page.svelte, fuera del innerHTML de #view,
  // así que render() nunca la desmonta ni reinicia su canvas WebGL.
  // Al cerrarse, el editor se desmonta y libera su contexto WebGL.
  import SkinEditor from './SkinEditor.svelte';

  let { onclose, initialSkinId = null, onEditorSave = () => {} }: {
    onclose: () => void;
    initialSkinId?: string | null;
    onEditorSave?: (info: { skinId: string; model: 'classic' | 'slim' }) => Promise<void>;
  } = $props();
</script>

<div class="se-overlay" role="dialog" aria-modal="true" aria-label="Editor de skins">
  <div class="se-bar">
    <span class="se-title">Editor de skins <small>conectado a Mi cuenta</small></span>
    <button class="se-close" type="button" onclick={onclose}>✕ Cerrar</button>
  </div>
  <div class="se-body">
    <SkinEditor
      title="Editor de skins — Obsidia Custom"
      subtitle="Editando: Explorador azul"
      {initialSkinId}
      onsave={(s) => void onEditorSave({ skinId: s.skinId, model: s.model })}
      onselect={() => {}}
      onnew={() => {}}
      onimport={() => {}}
      onexport={() => {}}
      oncolor={() => {}}
    />
  </div>
</div>

<style>
  .se-overlay {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: flex;
    flex-direction: column;
    background: #0b0a10;
  }
  .se-bar {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 14px;
    background: #121019;
    border-bottom: 1px solid rgba(255, 255, 255, 0.075);
    color: #f2f0f7;
    font: 600 14px/1 Inter, system-ui, sans-serif;
  }
  .se-bar small {
    font-weight: 500;
    color: #a09cb2;
    margin-left: 8px;
  }
  .se-close {
    border: 1px solid rgba(139, 92, 246, 0.55);
    background: rgba(139, 92, 246, 0.18);
    color: #f2f0f7;
    border-radius: 9px;
    padding: 8px 16px;
    font: 600 13px/1 Inter, system-ui, sans-serif;
    cursor: pointer;
  }
  .se-close:hover {
    background: rgba(139, 92, 246, 0.35);
  }
  .se-body {
    flex: 1;
    min-height: 0;
    min-width: 0;
  }
</style>
