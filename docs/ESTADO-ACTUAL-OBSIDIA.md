# Estado actual de Obsidia Launcher

## Arquitectura
- Frontend: SvelteKit + Tauri.
- Backend: Rust (Tauri commands).
- Perfiles con instancia aislada.
- Resource packs/shaders/mods instalados por perfil.

## Funcionalidades implementadas
- Modal de eliminación de perfil con texto ajustado.
- Instalados arriba con imagen y botón eliminar.
- Bloqueo de navegador (contextmenu, F5, Ctrl+R).
- Prevención de múltiples instancias de Minecraft.
- Eliminación segura de perfiles.
- Sincronización de pantalla completa con options.txt.
- Imágenes de Modrinth (icon_url) y CSP autorizada.
- Bloques 3D locales para perfiles.
- Memoria RAM total del sistema.
- Categorías de mods de Modrinth.

## Pendientes
- Steve auténtico como fallback.
- Java automático.
- Quilt/NeoForge y versiones específicas de loaders.
- Algunas mejoras de estado visual.

## Pruebas
- npm run build: OK
- cargo test --lib: OK (67 tests)
- cargo clippy --lib: OK (warnings preexistentes)
- Release build: OK (ejecutable generado)

## Protección
- Sin secretos publicados.
- Imágenes de bloques excluidas por licencia pendiente.
