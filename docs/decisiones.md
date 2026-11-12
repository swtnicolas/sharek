# Decisiones de ShareK

ShareK captura, graba y anota la pantalla. Tiene dos apps: una para KDE Plasma 6 en Wayland, en `linux/`, y una app de barra de menú para macOS, en `macos/`. Las dos hacen lo mismo, se llaman igual y dicen lo mismo, y el editor tiene el aspecto del de Linux en las dos. Cada diferencia está escrita en su sección, como "Solo en Linux" o "Solo en macOS", con su razón. Ante una diferencia nueva gana la versión más correcta, y se lleva a la otra plataforma.

## Objetivo

Capturar una región, una ventana o la pantalla completa, anotarla y copiarla o guardarla en pocos segundos. Además: grabar MP4 y GIF, capturar contenido más largo que la pantalla desplazándolo, fijar imágenes sobre las demás ventanas, medir, tomar colores y extraer texto.

## Nombre

El nombre visible es **ShareK**; el repositorio es `sharek`. El id es `local.sharek` en las dos plataformas:

- **Linux:** el archivo `.desktop` y el icono instalado son `local.sharek`, el servicio de D-Bus es `local.ShareK` y los atajos quedan en el componente `local_sharek` de KDE.
- **macOS:** el bundle es `local.sharek` y el aviso que cierra la app es `local.sharek.quit`.

Las capturas se guardan en la carpeta de imágenes del usuario, en `ShareK`, con nombres como `ShareK_2026-11-10_14-05-33.png`.

## Prioridad

Consumo mínimo de recursos, con la RAM por delante. En Linux el proceso de la bandeja ocupa como mucho 6 MB en reposo y el editor, el codificador de GIF y el OCR viven solo mientras tienen trabajo. En macOS el proceso de la barra de menú ocupa unos 8 MB en reposo; el editor y el OCR corren como procesos aparte que viven solo mientras tienen trabajo. En las dos, cada captura existe una sola vez en cada proceso: la pantalla congelada se muestra sin copiarla, un solo PNG sirve para el portapapeles y para el archivo, y el editor dibuja solo la parte visible al tamaño de la ventana. En las dos, las imágenes abiertas en el editor viven en archivos temporales sin nombre y no en la memoria del proceso, y solo la pestaña a la vista tiene interfaz; así abrir más capturas, o capturas más grandes, casi no cambia la memoria del editor. Esos archivos desaparecen al cerrar la captura o el editor. Ninguna de las dos sube nada a internet.

- **Solo en Linux:** la memoria del editor depende sobre todo del tamaño de su ventana, porque Slint entrega cada cuadro al compositor desde la memoria del proceso.
- **Solo en macOS:** el cuadro que se muestra, y la imagen de cada captura fijada, también viven en un archivo temporal, que el servidor de ventanas lee directamente; el editor ocupa unos 20 MB con cualquier tamaño de ventana y cualquier cantidad de pestañas. Una pestaña que no está a la vista guarda su captura comprimida. Todos los procesos usan el modo compacto del asignador de memoria del sistema (`MallocSpaceEfficient`).

## Equipos

- **Linux:** CachyOS con KDE Plasma 6.8 en Wayland, pantallas a escala 1 y 1,3.
- **macOS:** Mac con Apple Silicon y macOS 27.

## Stack

- **Linux:** Rust. Un proceso de bandeja con los atajos, la selección, los pines, el portapapeles y las grabaciones; un editor en Slint; un codificador de GIF y un OCR como programas aparte. Las capturas salen de `org.kde.KWin.ScreenShot2` y las grabaciones de `zkde_screencast` con GStreamer.
- **macOS:** Swift con AppKit, sin Xcode. Un proceso de barra de menú con los atajos, la selección, los pines, el portapapeles y las grabaciones; el editor y el OCR son el mismo ejecutable lanzado como procesos aparte. Las capturas y las grabaciones salen de ScreenCaptureKit.

## Capturas

Región, pantalla completa y ventana activa. La selección congela la pantalla, dibuja el contorno punteado en los cuatro lados, muestra una lupa junto al cursor y una etiqueta con los píxeles, la proporción y la parte de la pantalla. Al pasar sobre una ventana la propone como región.

- **Solo en Linux:** la pantalla completa es la pantalla activa de KWin, la que entrega `ScreenShot2`.
- **Solo en macOS:** la pantalla completa es la que está bajo el cursor.

Cada captura se copia, se guarda o se abre en el editor según los ajustes. Si los tres están apagados, la captura se abre en el editor: una captura nunca se pierde.

## Regla

Arrastrar mide ancho, alto, diagonal y ángulo; ⌃C o ⌘C copia la medida y Esc cierra. La etiqueta queda del lado contrario a la lupa, para que no se tapen. Los números van sin separador de miles.

- **Solo en Linux:** con escala distinta de 1 agrega el tamaño en "px lógicos".
- **Solo en macOS:** agrega el tamaño en "pt", la unidad de macOS.

## Editor

### Disposición

Una barra de una fila: Copiar (botón grande, que también se arrastra para soltar la captura en otra app), Guardar, Guardar como, Extraer texto, Extraer texto de una zona y Fijar; Deshacer y Rehacer; las herramientas en cuatro grupos (Editar, Dibujar, Texto, Efectos); Girar, Voltear y Marco; y Descartar a la derecha. Si la ventana es angosta, los grupos se pliegan en un botón con menú, primero Efectos, después Texto, Editar y Dibujar.

La ventana se abre del tamaño de la captura, al menos tan ancha como la barra sin plegar y como mucho el 85 % de la pantalla. Las opciones de la herramienta van en una píldora flotante abajo, que empieza con el nombre de la herramienta o de la forma seleccionada; el zoom va abajo a la derecha. La píldora nunca tapa el zoom: si centrada lo toca, se corre a la izquierda; si tampoco cabe al lado, el zoom sube sobre ella; y si ni así cabe, se ocultan el nombre y las etiquetas de los colores, que siguen a la vista como título del selector de color. Después de usar un control de la píldora, el teclado vuelve al lienzo, o al texto si se está escribiendo. Al pasar el puntero sobre un botón aparece un recuadro con el nombre, el atajo y una explicación corta.

Todas las capturas y grabaciones abiertas comparten una ventana. Con más de una, sobre la barra aparece una barra de pestañas propia: cada pestaña muestra su título y un punto si tiene cambios sin guardar; la que está a la vista lleva arriba una línea del color de acento, y esa y la que está bajo el puntero muestran el botón de cerrar. Cerrar una pestaña deja a la vista la siguiente, o la anterior si era la última; cerrar la ventana las cierra todas.

### Herramientas

Seleccionar (M), Recortar (C), Quitar franja (X), Borrador inteligente (Y); Rectángulo (R), Elipse (E), Línea (L), Flecha (A), Mano alzada (F), Resaltador (H); Texto con contorno (O), Texto con fondo (T), Globo de diálogo (S), Numeración por pasos (I); Desenfoque (B), Pixelado (P), Foco (K), Lupa (G). La letra elige la herramienta solo sin teclas modificadoras.

### Tamaños por defecto

Los tamaños son fijos: lo que se elige es lo que se dibuja, sin importar el tamaño de la imagen.

| Herramienta | Grosor | Tamaño de texto | Esquinas |
|---|---|---|---|
| Rectángulo, elipse, línea, mano alzada | 2 px | — | 0 px |
| Flecha | 3 px | — | — |
| Texto con contorno | contorno de 2 px | 16 | — |
| Texto con fondo | — | 14 | 4 px |
| Globo de diálogo | borde de 2 px | 14 | 6 px |
| Numeración por pasos | borde de 2 px | 12 | — |
| Lupa | 2 px | — | — |

El desenfoque usa 15 px, el pixelado 10 px, la lupa 2× y el foco oscurece un 60 %. Las formas vienen en rojo `#F23C3C` y los números llevan el texto blanco sobre rojo. Los textos son negros: el de contorno con contorno blanco y sin sombra, el de fondo sobre blanco con sombra, y el globo sobre blanco con borde gris `#8E8E93` y sombra. Cada cambio de estilo se recuerda para la próxima forma de esa herramienta.

### Iconos

Los iconos del editor son los mismos dibujos en las dos plataformas: trazos de 1,9 px sobre una cuadrícula de 24, de 22 px de lado. Lo mismo vale para el icono de Copiar, la flecha de los grupos plegados, el cuentagotas, el ajuste del zoom y la insignia de video del historial; la de GIF es el texto "GIF". Durante una grabación, el icono de la bandeja o de la barra de menú es un disco rojo `#FF3B30` con un cuadrado blanco.

Las herramientas de dibujo muestran el color que tienen elegido, y las que tienen relleno lo muestran dentro del icono; un color demasiado oscuro para la barra se muestra gris, salvo en el texto con contorno, que muestra el color del contorno. El resto toma un color fijo según lo que hace:

| Grupo | Color |
|---|---|
| Guardar, Guardar como | azul `#5B9BFF` |
| Extraer texto | turquesa `#2EC4B6` |
| Fijar | naranja `#FF9F1C` |
| Deshacer, Rehacer | ámbar `#FFB020` |
| Seleccionar | blanco `#E8EAED` |
| Recortar, Quitar franja, Girar, Voltear, Marco, Abrir en el reproductor | verde `#34C759` |
| Borrador inteligente | rosa `#FF7AB6` |
| Desenfoque, Pixelado, Foco | morado `#AF7BFF` |
| Mostrar en la carpeta | amarillo `#FFC94D` |
| Descartar | rojo `#FF6B6B` |

### Colores de las formas

Dieciséis muestras (rojo, naranja, amarillo, verde, turquesa, azul, morado, rosa, blanco, gris claro, gris, gris oscuro, pizarra, negro, amarillo flúor y transparente) y "Tomar color de la imagen", que toma el color del punto donde se hace clic, formas incluidas.

- **Solo en macOS:** "Otro color…" abre el panel de colores del sistema; Slint no tiene uno en Linux.

### Texto

Un clic con una herramienta de texto crea el texto y se escribe de inmediato; cada clic en otro lugar termina el texto actual y crea otro. El doble clic sobre un texto lo edita con cualquier herramienta, e Intro edita el texto seleccionado. Esc termina la edición.

Mientras se escribe, el texto se ve como quedará: colores, contorno, fondo, borde, sombra y cola del globo. El recuadro de edición es el mismo contorno de la selección de formas, el cursor es negro con borde blanco y el texto seleccionado se marca en azul translúcido.

Ctrl+Z (⌘Z) deshace primero lo escrito; cuando el texto vuelve a como empezó, termina la edición y deshace el paso anterior del documento. Ctrl+Mayús+Z (⇧⌘Z) rehace lo escrito si se escribió algo y, si no, rehace en el documento.

### Formas

La forma seleccionada se marca con un contorno punteado blanco sobre negro, que se ve sobre cualquier fondo y no se confunde con los colores de las herramientas. En las formas que se redimensionan por las esquinas (rectángulo, elipse, lupa, desenfoque, pixelado, resaltador, foco, borrador inteligente e imagen pegada) el contorno pasa por el centro de las asas, que son blancas con borde negro. Las demás formas llevan el contorno un poco por fuera. El recorte y la zona de texto a extraer usan el mismo contorno.

El clic derecho sobre una forma la selecciona y abre Duplicar, Traer al frente, Enviar al fondo y Eliminar. Las flechas mueven la forma 1 px, o 10 px con Mayús.

El resaltador tiñe todo lo que tiene debajo, imagen y formas, multiplicándolo por su color. El desenfoque y el pixelado toman solo la imagen y tapan las formas que cubren.

### Girar, voltear y marco

Girar 90° (con Mayús, al revés) y Voltear (con Mayús, de arriba abajo) cambian la imagen y mueven las formas; los textos conservan su tamaño y quedan derechos. El marco agrega margen (0, 16, 32, 48 o 64 px), fondo, esquinas (0, 6, 12, 18 o 24 px), borde (0, 1, 2, 4 u 8 px) y sombra; por defecto 32 px de margen, fondo blanco, esquinas de 12 px y sombra. Las tres operaciones se deshacen como un solo paso.

### Zoom

Menos, porcentaje, más y ajustar; Ctrl+rueda o ⌘ con el pellizco. La vista se ajusta a la ventana al abrir y al redimensionar hasta que el usuario hace zoom.

### Pegar

Pegar una imagen la agrega como forma; si el portapapeles no tiene una imagen, aparece un aviso.

## Grabación

MP4 a 30 fps y GIF a 15 fps de una región. Un borde rojo punteado rodea la región por fuera, y un panel muestra el formato, el tiempo, Detener y Cancelar; el panel se arrastra desde cualquier punto que no sea un botón. La misma tecla que empieza la grabación la detiene. Si al detenerla la grabación no termina en 15 s, se cierra con un aviso. Salir de la app durante una grabación la cancela y borra el archivo a medias.

- **Solo en Linux:** si la región llena la pantalla, el panel queda dentro y sale en el video, porque `zkde_screencast` no permite excluir ventanas. El video es H.264 con x264 en la CPU, porque el codificador de NVIDIA carga cientos de MB.
- **Solo en macOS:** el borde y el panel quedan fuera del video. El video es H.264, o HEVC si supera 4096 px.

## Captura con desplazamiento

El panel dice "Desplazamiento" y desplaza solo la zona elegida: empieza con un paso pequeño, mide cuánto se movió el contenido y ajusta los siguientes pasos para avanzar un 40 % de la zona cada vez. Antes de cada captura espera a que la pantalla se quede quieta; una pantalla quieta con algo animado cuenta como quieta. Una unión se acepta cuando coincide el 90 % del solape, comparado en bloques de 8 píxeles, así que un GIF o un emote animado no la impiden. El resaltado que una página pinta bajo el cursor tampoco: en modo automático las franjas alteradas que tocan la fila del cursor no cuentan, y se toleran hasta dos franjas alteradas más si juntas no pasan de un tercio del solape. Las columnas que no se mueven, como una barra lateral fija, no cuentan, y lo fijo de abajo nunca entra en una unión. Termina sola al llegar al final.

Al empezar, el cursor se lleva una sola vez al centro de la zona; desde ahí queda libre y el desplazamiento ocurre donde esté. Sacar el cursor de la zona es tomar el control: la captura pasa al modo manual y no envía más desplazamientos.

El botón Manual del panel, sacar el cursor de la zona o cuatro pasos seguidos sin solape pasan al modo manual: el panel dice "Desplázate" y la captura sigue lo que el usuario desplaza; si va demasiado rápido dice "Más despacio" hasta que vuelve a haber solape. El modo manual termina con Detener o con el atajo. Cancelar muestra "Procesando…" y no entrega nada. Las ventanas de ShareK, pines incluidos, no salen en la captura.

- **Solo en Linux:** el desplazamiento automático usa la entrada simulada de KWin, que solo concede a los programas instalados. Wayland no deja leer la posición del cursor, así que mientras dura el modo automático una capa invisible cubre las pantallas fuera de la zona: detecta que el cursor salió y recibe cualquier desplazamiento enviado en ese instante, para que no mueva otra ventana.
- **Solo en macOS:** el desplazamiento automático necesita el permiso de Accesibilidad; sin él la captura empieza en modo manual.

## Pines

Una imagen fijada flota sobre las demás ventanas. Se mueve arrastrándola, la rueda cambia el tamaño y la rueda con Alt (u Opción) la opacidad, hasta 15 %. El pellizco del touchpad también cambia el tamaño. Esc, Retroceso, Supr o el doble clic la cierran; el intervalo del doble clic es el del sistema. El clic derecho abre un menú con Copiar, Abrir en el editor, Guardar como…, Extraer texto (OCR), la opacidad, Tamaño original, Cerrar y Cerrar todas.

## OCR

Extrae el texto de una región al portapapeles, o de una captura en el editor (toda la imagen o una zona que se arrastra) a una ventana desde donde se copia todo o la parte seleccionada.

- **Solo en Linux:** usa los modelos PP-OCRv5 sobre ONNX Runtime, en el equipo.
- **Solo en macOS:** usa Vision, del sistema.

## Selector de color

Copia el color en hexadecimal y muestra RGB y HSB en un aviso. Durante una grabación o una captura con desplazamiento, el selector y la regla no se abren.

- **Solo en Linux:** congela la pantalla y usa la lupa de la selección.
- **Solo en macOS:** usa el selector de colores del sistema.

## Historial

Hasta 1000 archivos guardados, con miniaturas, en una ventana que recuerda su tamaño. Los archivos borrados desaparecen de la lista al abrirla.

## Portapapeles

Guarda las últimas 5 capturas copiadas y lo que había antes de la primera. Descartar una captura que sigue en el portapapeles vuelve a la anterior. Las capturas se ofrecen como PNG.

## Bandeja y menú

Capturar región, Capturar pantalla completa, Capturar ventana activa, Captura con desplazamiento, Capturar texto (OCR); Fijar región en pantalla, Fijar imagen del portapapeles, Cerrar imágenes fijadas; Grabar video (MP4), Grabar GIF; Selector de color, Regla, Historial…; los ajustes; Elegir carpeta de guardado…, Abrir carpeta de guardado; Salir de ShareK. Durante una grabación el icono pasa a un botón rojo de detener y el menú empieza con Detener.

- **Solo en Linux:** el menú muestra las teclas que KDE tiene guardadas, y el globo del icono muestra la versión.

## Atajos

| Acción | Linux | macOS |
|---|---|---|
| Capturar región | Impr | ⇧⌘4 |
| Capturar pantalla completa | Mayús+Impr | ⇧⌘3 |
| Capturar ventana activa | Meta+Impr | ⇧⌘5 |
| Grabar video (MP4) | Ctrl+Mayús+6 | ⌃⇧6 |
| Grabar GIF | Ctrl+Mayús+7 | ⌃⇧7 |
| Captura con desplazamiento | Ctrl+Mayús+8 | ⌃⇧8 |
| Fijar región en pantalla | Ctrl+Mayús+9 | ⌃⇧9 |
| Capturar texto (OCR) | Ctrl+Mayús+0 | ⌃⇧0 |
| Selector de color | Ctrl+Mayús+1 | ⌃⇧1 |
| Regla | Ctrl+Mayús+2 | ⌃⇧2 |
| Historial | Ctrl+Mayús+H | ⌃⇧H |

Si un atajo no se puede registrar, aparece un aviso.

- **Solo en Linux:** los atajos van por el portal de atajos globales y se cambian en Configuración del sistema › Atajos. Mac no tiene tecla Impr.
- **Solo en macOS:** los atajos son fijos, y ⇧⌘3, ⇧⌘4 y ⇧⌘5 funcionan solo si se desactivan los de capturas de macOS.

## Ajustes

| Ajuste | Valor por defecto |
|---|---|
| Copiar al portapapeles al capturar | Sí |
| Guardar automáticamente | No |
| Abrir editor al capturar | Sí |
| Incluir cursor en capturas | No |
| Grabar audio del sistema | No |
| Abrir al iniciar sesión | Sí, la primera vez que se abre |
| Carpeta de guardado | Imágenes › ShareK |

- **Solo en Linux:** los ajustes van en `~/.config/sharek`.
- **Solo en macOS:** los ajustes van en las preferencias del sistema, `local.sharek`.

## Avisos

Los errores aparecen como un aviso que no interrumpe ni roba el foco.

- **Solo en macOS:** la falta del permiso de Grabación de pantalla abre una alerta, porque sin él la app no puede hacer nada.

## Icono

Un remolino de cuatro hojas (rosa `#FF3D7F`, naranja `#FF9F1C`, turquesa `#2EC4B6` y azul `#5B5BFF`) sobre un recuadro oscuro redondeado. El maestro es `assets/icon/AppIcon.svg`. El icono de la bandeja o de la barra de menú es el mismo remolino, plano y sin recuadro.

## Formato

- PNG o JPEG, con la escala de la pantalla guardada en el archivo para que al abrirlo conserve su tamaño.
- El tamaño de un archivo se muestra en KB por debajo de 1 MB y en MB con un decimal desde ahí.

## Colores

El editor es oscuro: ventana `#1B1D20`, barra `#26292D`, lienzo `#131517`, recuadros de ayuda y opciones `#2F3338`, bordes `#3F444B`, texto `#EFF0F1`, texto secundario `#C3C8CE` y el acento `#FF3D7F` en el botón Copiar y en las pestañas.

## Permisos

- **Linux:** KWin concede la captura, la grabación, la entrada simulada y la lista de ventanas solo a los programas que su archivo `.desktop` declara, así que solo funcionan los programas instalados.
- **macOS:** Grabación de pantalla para capturar y grabar, y Accesibilidad para el desplazamiento automático. macOS ata los permisos a la firma; el certificado "Nico Local" los conserva entre compilaciones.

## Instalación

- **Linux:** un instalador descargable deja los programas en `~/.local/bin`, descarga los modelos de OCR y arranca ShareK. Se publica con cada tag `linux-v<versión>`.
- **macOS:** la app se compila en el propio Mac, porque macOS bloquea una copia descargada sin firma de Apple, y se instala en `~/Applications`.
