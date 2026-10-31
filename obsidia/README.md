# Obsidia — launcher de Minecraft Java

Launcher propio para Windows 10, inspirado en SKLauncher.
Objetivo: de los más bonitos visualmente y de los más optimizados.

## Identidad visual
- Fondo negro azulado `#0A0A12`
- Acento violeta `#8B5CF6` (portal del Nether) y magenta `#C026D3`
- Cian `#22D3EE` solo en lo activo (botón JUGAR, barras de progreso)
- Paneles de vidrio esmerilado, bordes finos, animaciones suaves
- Logo: esquirla de obsidiana facetada
- Tipografías: Sora (títulos) e Inter (texto)

## Stack propuesto
- Tauri 2 (Rust + WebView2): liviano, poca RAM
- Rust en el backend: descargas paralelas, hashes, lanzamiento del juego
- Svelte + TypeScript + Tailwind en la interfaz

## Contenido de este zip
- `prototipo/obsidia-prototipo.html`: prototipo clickeable (abrilo en el navegador).
  Pantallas: Inicio, Perfiles, Mods (con pestañas Mods, Shaders y Resource packs),
  Servidores, Ajustes y Mi cuenta.
- `maquetas-canvas/`: las dos maquetas estáticas del diseño (Inicio y Mods).
  Son archivos del lienzo de diseño de Claude, no se abren solos en el navegador.

## Qué hace el prototipo (todo simulado, no descarga ni lanza nada real)
- JUGAR con progreso de preparación; perfiles activos y creación de perfiles nuevos
- Mods, shaders y resource packs: buscar, filtrar, instalar y eliminar (tacho con confirmación)
- Shaders: etiqueta de rendimiento (Ligero, Medio, Pesado) y aviso de que requieren Iris
- Resource packs: panel de prioridad con interruptor, flechas ↑ ↓, tacho,
  aviso si un pack base queda encima de un retoque y botón "Ordenar automático"
- Servidores: ping, jugadores, aviso de versión distinta al perfil, añadir y quitar
- Ajustes: RAM, versión de Java, modo ligero (apaga animaciones), consola
- Mi cuenta: nombre de jugador con validación, skins de ejemplo, login Microsoft simulado

## Hitos de programación
1. Lanzar vanilla offline desde un script — ✅ `scripts/launch_vanilla.py` (o `.bat`)
2. Descargas automáticas + selector de versión — ✅ Tauri (list_versions + launch)
3. Perfiles + gestor de Java — ✅ Tauri (profiles + detect_javas)
4. Fabric y Forge — ✅ Tauri (install_fabric/install_forge)
5. Interfaz completa — ✅ tabs funcionales (Inicio, Perfiles, Mods, Servidores, Ajustes, Cuenta)
6. Login Microsoft + Modrinth — ⚠️ Modrinth ✅, Microsoft pendiente (offline por ahora)
7. Instalador y autoactualización — ✅ Instalador NSIS/MSI + updater firmado (`target/release/bundle`)

## Pendiente
- Datos reales en la interfaz (todo lo del prototipo es de ejemplo)
- Avisar de dependencias al borrar un mod (por ejemplo Fabric API)
- Verificar que el nombre "Obsidia" no esté usado por otro launcher
