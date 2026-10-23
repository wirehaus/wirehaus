# Contribuir

Gracias por el interés. Wirehaus está en desarrollo temprano y la arquitectura todavía se mueve, así que antes de escribir código abre un issue y cuéntame qué quieres cambiar.

## Antes de enviar un cambio

```
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Los tres tienen que pasar sin avisos.

## Normas del código

- Sin comentarios. Si algo necesita una explicación, el nombre o la estructura no son lo bastante claros.
- Funciones pequeñas y nombres precisos, en inglés.
- Ningún `unsafe`: el proyecto lo prohíbe.
- Una dependencia nueva necesita una razón. Explícala en la descripción del cambio.
- Cada cambio de comportamiento lleva su test.

## Commits

Un cambio lógico por commit. Mensaje en inglés, en imperativo y en una línea corta, con cuerpo si hace falta explicar el porqué.

## Seguridad

Las vulnerabilidades no se reportan en issues públicos. Escribe a [security@wirehaus.dev](mailto:security@wirehaus.dev) como indica [SECURITY.md](SECURITY.md).

Para dudas antes de enviar un cambio, escribe a [contact@wirehaus.dev](mailto:contact@wirehaus.dev).

## Licencia

Al enviar un cambio aceptas que se distribuya bajo la licencia del proyecto, [PolyForm Shield 1.0.0](LICENSE).
