# ◈ Obsidia Launcher

### Tu Minecraft Java, tus perfiles, tus reglas.

Obsidia es un launcher de escritorio para **Minecraft Java Edition**, diseñado para simplificar la gestión de perfiles, loaders, mods, shaders y resource packs desde una interfaz moderna.

El objetivo es ofrecer una experiencia sencilla y organizada: crear distintas instancias de Minecraft, personalizarlas y administrar su contenido sin tener que gestionar manualmente cada instalación.

<p align="center">
  ![Platform](https://img.shields.io/badge/platform-Windows-0078D6?logo=windows&logoColor=white)
  ![Tauri](https://img.shields.io/badge/Tauri-2-FFC131?logo=tauri&logoColor=white)
  ![Rust](https://img.shields.io/badge/backend-Rust-DEA584?logo=rust&logoColor=white)
  ![Svelte](https://img.shields.io/badge/frontend-Svelte-FF3E00?logo=svelte&logoColor=white)
  ![Status](https://img.shields.io/badge/status-in%20development-orange)
</p>

---

## ✨ ¿Qué ofrece Obsidia?

### 🎮 Perfiles independientes

Creá diferentes perfiles para organizar tus instalaciones de Minecraft.

Cada perfil puede tener su propia configuración:

- Versión de Minecraft.
- Loader y versión del loader.
- Memoria RAM asignada.
- Configuración de Java.
- Pantalla completa.
- Personalización visual del perfil.

Cada instancia mantiene sus propios archivos de juego, evitando mezclar mods y configuraciones entre perfiles.

### 🧩 Catálogo de mods

Obsidia se integra con [Modrinth](https://modrinth.com/) para buscar e instalar contenido directamente desde el launcher.

Desde la aplicación podés explorar:

- Mods.
- Shaders.
- Resource packs.

También podés consultar los elementos instalados y eliminarlos desde la propia interfaz.

### ⚙️ Gestión de loaders

Obsidia incorpora soporte para:

- Vanilla.
- Fabric.
- Forge.

La gestión de loaders permite configurar perfiles con distintas combinaciones de Minecraft y loader.

La ampliación del soporte a otros loaders forma parte del desarrollo futuro.

### ☕ Gestión de Java

El launcher puede detectar instalaciones de Java y gestionar los runtimes necesarios para ejecutar Minecraft.

La selección de Java tiene en cuenta los requisitos de la versión del juego, evitando depender exclusivamente de una instalación manual del usuario.

### 🔐 Cuenta y autenticación

Actualmente, Obsidia permite iniciar Minecraft en modo offline utilizando un nombre de jugador local.

La compatibilidad con cuentas Microsoft que tengan una licencia de Minecraft Java todavía no está disponible en la versión actual y queda como objetivo de desarrollo.

### 🌐 Servidores

Administrá tus servidores desde el launcher:

- Guardar servidores.
- Editar y eliminar entradas.
- Consultar su disponibilidad mediante ping.
- Acceder al servidor seleccionado al iniciar Minecraft.

### 🎨 Resource packs y shaders

Gestioná tus recursos visuales desde una misma sección.

Los resource packs permiten organizar su prioridad y activar o desactivar elementos desde la interfaz.

---

## 🛠️ Tecnologías

Obsidia está desarrollado con tecnologías orientadas a crear una aplicación de escritorio ligera y moderna.

| Tecnología | Uso |
|---|---|
| [Tauri 2](https://v2.tauri.app/) | Aplicación de escritorio |
| [Rust](https://www.rust-lang.org/) | Backend y operaciones del launcher |
| [Svelte 5](https://svelte.dev/) | Interfaz de usuario |
| [SvelteKit](https://svelte.dev/docs/kit) | Estructura de la aplicación |
| [TypeScript](https://www.typescriptlang.org/) | Herramientas del frontend |
| [Vite](https://vite.dev/) | Desarrollo y compilación |
| [Modrinth API](https://docs.modrinth.com/) | Búsqueda y descarga de contenido |

---

## 📥 Descargar

Obsidia se encuentra actualmente en desarrollo.

Cuando haya versiones compiladas disponibles, se publicarán en:

**[GitHub Releases](https://github.com/lautarolopez748-wq/Obsidia-Launcher/releases)**

---

## 🚧 Desarrollo y próximos objetivos

Obsidia continúa evolucionando. Algunas de las áreas previstas son:

- Incorporar soporte para Quilt y NeoForge.
- Mejorar la organización de mods según el loader de cada perfil.
- Permitir recuperar colecciones de mods al cambiar entre loaders.
- Seguir mejorando la gestión de instancias y la experiencia de usuario.
- Ampliar las pruebas de compatibilidad y estabilidad.
- Incorporar y verificar el inicio de sesión con cuentas Microsoft/Minecraft premium.

Las funciones planificadas no deben considerarse disponibles hasta que estén implementadas y verificadas.

---

## 🤝 Contribuciones

¿Encontraste un error o tenés una idea para mejorar Obsidia?

Podés colaborar mediante:

- Reportes de errores.
- Sugerencias de funcionalidades.
- Comentarios sobre la experiencia de uso.
- Propuestas de mejora del código.

Para reportar un problema, incluí los pasos para reproducirlo y, cuando sea posible, los mensajes de error correspondientes.

**[Reportar un problema](https://github.com/lautarolopez748-wq/Obsidia-Launcher/issues)**

---

## 📌 Estado del proyecto

Obsidia es un proyecto en desarrollo activo. Algunas funcionalidades pueden cambiar entre versiones y todavía quedan mejoras de compatibilidad y estabilidad por incorporar.

El objetivo es construir un launcher que ofrezca una experiencia clara, organizada y flexible para jugar Minecraft Java.

---

## ⚖️ Aviso

Minecraft es una marca de Mojang Studios y Microsoft. Obsidia es un proyecto independiente y no está afiliado, respaldado ni mantenido por Mojang Studios o Microsoft.

Modrinth es un servicio de terceros. Su contenido y sus respectivos derechos pertenecen a sus autores.

**Licencia del proyecto:** todavía no se ha incorporado un archivo de licencia al repositorio.
