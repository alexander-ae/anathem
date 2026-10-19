# Insumos para la fase 2

Generado al cerrar la fase 1 (2026-09-22). El workspace de ejecución se borró;
esto es lo que sobrevive como registro accionable.

## Decisión D7 — sigue abierta, ahora con datos

```
léxicas       n=34   recall@50 = 100%   recall@5 = 88%
parafrásticas n=16   recall@50 =  62%   recall@5 = 19%
ranks de acierto parafrástico: 1, 2, 4, 10, 10, 15, 23, 36, 40, 43
```

BM25 resuelve las léxicas por completo. En parafrásticas el problema se parte:

- **7 casos**: el documento se encuentra y queda enterrado bajo el rank 5 → reranker.
  (10 están en el top 50, pero 3 ya están en el top 5, así que el margen real es 7.)
- **6 casos**: no aparece nunca en el top 50 → vectores o expansión de consulta.

El spec asumía empezar por la capa vectorial. La medición sugiere lo contrario.

## Residuos de medición (de la revisión final)

- Profundidad de recuperación no equiparable: `sm_baseline.py` pide limit=50 y
  deduplica después, así que 7 de 50 casos devolvieron solo 6-9 documentos.
  Arreglo barato: registrar `len(got)` por caso.
- p50 con dos estimadores distintos (mediana superior vs promediada).
- La latencia no es like-for-like: anathem es in-process, el sistema contra el
  que se comparó en la fase 1 corría por HTTP.
- El 5.4 ms publicado es de build debug; en release da 1.9 ms.

## Comandos que faltan

No existe `mem unsupersede` ni `mem unforget`. Eso hace irreversible desde la CLI
cualquier error al marcar. Es la razón principal por la que las 24 memorias con
corrección NO se vincularon automáticamente.

## Hallazgos diferidos

### Arreglar pronto

- Ruta de temporal predecible en `mem edit` (symlink-follow al escribir).
- El filtro por tags no tiene cobertura y su semántica AND no está documentada; está en la superficie MCP.
- Ningún test manda `+ - # "` por `to_match`, que es la razón entera de existir de esa función.
- No hay forma de buscar memorias no-`active`, así que `forget` es irreversible sin SQL.
- Una tool MCP desconocida devuelve success con texto, no un error JSON-RPC.
- `budget_used` se recalcula en `mcp.rs` en vez de reusar `est_tokens`; pueden derivar.
- `history` con id inexistente sale 0 sin imprimir nada.
- Re-migrar cuenta como `migrada` un documento ya presente.
- Sin test que fuerce un rollback parcial.
- `yaml_dq` no escapa DEL (0x7F) ni el rango C1 (0x80-0x9F). Cero ocurrencias hoy.
  Arreglo: `< 0x20 || c == 0x7f || (0x80..=0x9f).contains(&(c as u32))`.

### Se puede dejar

Dieciséis más, de bajo impacto: manejo de error en el script de export de un solo
uso, `resolve("~")` sin barra, `assert!` inalcanzable en `chunk::split`, rama
muerta `'A'..='F'` en `tags.rs`, `SELECT COUNT` por fila en la migración,
`limit==0`, el `##` arrastrado al `description`, entre otros.

## Limpieza post-migración pendiente

- Una memoria curada que quedó en status `failed` en el export de origen no se
  migra automáticamente (comportamiento esperado, no un bug): su contenido
  queda intacto en el JSONL; hay que reimportarla a mano con `mem save`.
- Las memorias que contienen una corrección son candidatas a `mem supersede`.
  El mecanismo está cableado y probado; cuál reemplaza a cuál es criterio de
  quien migra, no algo que se pueda automatizar con seguridad.

## Taxonomía

Una fracción del corpus real migrado en la fase 1 quedó sin ningún tag: la
taxonomía por defecto es deliberadamente pequeña y no pretende cubrir todo
(procedimientos operativos, decisiones de producto, y demás conocimiento
legítimo que no encaja en ninguna de las categorías de ingeniería). Desde la
limpieza para publicación, además, la taxonomía es reemplazable por completo
con un `tags.json` junto a la base de datos — ver el README — así que
extenderla o adaptarla al vocabulario de un equipo ya no requiere tocar el
binario.
