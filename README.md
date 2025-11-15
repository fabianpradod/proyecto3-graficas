# proyecto3-graficas

Simulacion en software del sistema para el Proyecto 3 de Graficas por Computadora.

## Como correrlo
1. Instala Rust (stable).
2. Desde la raiz del repo ejecuta:
   ```bash
   cargo run --release
   ```
3. Se abrira la ventana interactiva. Puedes alternar entre los dos temas con la tecla `T`.

## Controles
- `W / A / S / D`: mover la camara (y la nave) sobre el plano ecliptico.
- `Space` / `Left Shift`: elevar o descender (movimiento 3D).
- `← / →`: yaw de la camara.
- `↑ / ↓`: pitch de la camara.
- `1` a `5`: **teleport** animado al sol o a cada planeta.
- `T`: alternar entre los temas "Ice" y "Ember".
- `Esc`: salir.

La camara ahora es totalmente en tercera persona: la nave (modelada en `spaceship.obj`) se mantiene frente al visor, con un offset cinematografico para que la sigas observando mientras orbitas el sistema.

## Caracteristicas principales
- Dos temas completos (Ice y Ember) con colores, iluminacion y materiales distintos. Cada tema incluye 4 planetas de tamaños muy variados mas un sol.
- Un planeta luce un **anillo volumetrico** que se mantiene alineado con su eje mientras la orbita progresa.
- Planetas con orbitas y rotaciones independientes, renderizado de orbitas y warp instantaneo hacia cada cuerpo celeste.
- Renderer en CPU con iluminacion direccional, buffer de profundidad, skybox procedural y banda ecliptica adaptada al tema activo.
- Nave propia que sigue a la camara en tercera persona, colisiones para evitar atravesar cuerpos y controles 3D.
- Skybox con centenares de estrellas, tono adaptado al tema y estetica glacial/volcanica.

## Video
https://youtu.be/gNh5A4t9Y4g 

## Activos
- `spaceship.obj`: modelo de la nave que acompaña a la camara.
