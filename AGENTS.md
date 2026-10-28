# Instalar anathem

Si alguien te pidió que instales anathem en su máquina, esto es todo lo que
hay que hacer. No hace falta leer el resto del repositorio.

## El camino corto

```bash
curl -fsSL https://gitlab.com/__alexander__/anathem/-/raw/main/scripts/instalar.sh -o instalar.sh
sh instalar.sh --configurar-path --conectar
```

Eso baja el binario ya compilado de la plataforma, verifica su sha256, lo
instala en `~/.local/bin/mem`, agrega ese directorio al `PATH` de la shell,
crea la base en `~/.anathem/mem.db`, baja el paquete de vocabulario en-es
(22 MB), registra anathem como servidor MCP en Claude Code y termina con un
diagnóstico. Es idempotente: correrlo de nuevo no rompe nada.

No necesita Rust. Hay binarios para Linux x86_64, macOS arm64, macOS x86_64 y
Windows x86_64; en cualquier otra plataforma el script compila desde fuente y
ahí sí pide Rust y un compilador de C.

## Lo que el script no puede hacer solo

**opencode, en algunos casos.** El script lo configura solo: busca
`~/.config/opencode/opencode.jsonc`, después `.json`, y respeta
`OPENCODE_CONFIG` si está definida. Inserta una línea dentro de la clave
`"mcp"` en vez de reescribir el archivo —así no se pierden los comentarios— y
deja una copia en `.bak-anathem`.

Se abstiene, y te imprime la entrada, cuando el archivo tiene una forma que no
puede editar sin riesgo: sin clave `"mcp"`, con la clave escrita de forma
compacta, o con más de una. En ese caso agregala vos, dentro de su `"mcp"`, sin
tocar lo demás:

```jsonc
"anathem": {
  "type": "local",
  "command": ["/home/USUARIO/.local/bin/mem", "--db", "/home/USUARIO/.anathem/mem.db", "serve"],
  "enabled": true
}
```

Reemplazá la ruta por la que el script haya impreso.

**Claude Code sin el CLI.** El registro usa `claude mcp add --scope user`. Si
el comando `claude` no está en el `PATH` —pasa cuando solo está la aplicación
de escritorio— el script lo dice y sigue. Ahí hay que agregar a mano la entrada
`mcpServers` que describe [`docs/instalacion.md`](docs/instalacion.md).

**Reiniciar el agente.** Las tools nuevas aparecen en una sesión nueva, no en
la que está corriendo. Decíselo al terminar.

## Comprobar que quedó bien

```bash
mem doctor
```

Tiene que terminar en `Todo en orden.` y salir con 0. Los códigos de salida
son 0 sin problemas, 1 con problemas, 2 si el diagnóstico no pudo correr.

Después, en una sesión nueva del agente, tienen que aparecer cuatro tools:
`search_memory`, `save_memory`, `get_memory` y `list_tags`. Para confirmarlo
de verdad, guardá algo y buscalo:

```bash
mem save "una nota de prueba"
mem search "prueba"
```

## Si algo falla

- **`mem: command not found`** tras instalar: el `PATH` no se recargó. Abrí
  una terminal nueva, o `export PATH="$HOME/.local/bin:$PATH"`.
- **macOS bloquea el binario**: puede pasar si lo bajaron con el navegador en
  vez del script, porque Gatekeeper marca esas descargas. Se destraba con
  `xattr -d com.apple.quarantine ~/.local/bin/mem`.
- **El diagnóstico se queja de la capa vectorial**: la herramienta funciona
  igual, solo con búsqueda léxica. El paquete se instala después con
  `mem model install <archivo.anathem-pack>`.

Para todo lo demás, incluida la instalación paso a paso y cómo migrar un
corpus existente, ver [`docs/instalacion.md`](docs/instalacion.md).
