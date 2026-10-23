## Qué cambia y por qué

## Comprobaciones

- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` y `cargo test --workspace` pasan
- [ ] Hay un test por cada cambio de comportamiento
- [ ] Sin comentarios en el código ni en las configuraciones
- [ ] Sin `unsafe` y sin dependencias nuevas, o la razón está explicada arriba
- [ ] Si toca la seguridad, el modelo de amenazas está actualizado
