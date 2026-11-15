<div align="center">
  <img src="images/omenspace.png" alt="Logo de OMEN Space" width="110" />

  # OMEN Space · Edición 8C75

  **Fork de [OMEN Space](https://github.com/yunusemreyl/omen-space) optimizado para el HP OMEN 17-db0xxx (placa `8C75`) en Linux.**  
  Refrigeración silenciosa y estable, controles de energía fieles al hardware y un escritorio sincronizado con KDE Plasma, todo funcionando por eventos.

  <p>
    <img src="https://img.shields.io/badge/Placa-HP%208C75-e11d48?style=flat-square" alt="Placa 8C75" />
    <img src="https://img.shields.io/badge/Basado%20en-OMEN%20Space%202.2.1-6366f1?style=flat-square" alt="Basado en OMEN Space 2.2.1" />
    <img src="https://img.shields.io/badge/Uso%20diario-Nobara%2044%20%C2%B7%20KDE%20Plasma%206-0ea5e9?style=flat-square" alt="Nobara 44 · KDE Plasma 6" />
    <img src="https://img.shields.io/badge/Lenguaje-Rust-f97316?style=flat-square" alt="Rust" />
    <a href="LICENSE"><img src="https://img.shields.io/badge/Licencia-GPL--3.0-10b981?style=flat-square" alt="Licencia GPL-3.0" /></a>
  </p>

  <p>
    <a href="README.md">English</a> · <b>Español</b>
  </p>
</div>

---

## Por qué este fork

OMEN Space es compatible con una larga lista de placas HP OMEN, Victus y Transcend. En el **OMEN 17-db0xxx (8C75)** varias cosas no se comportaban como el código genérico espera. La más grave: el controlador embebido (EC) nunca vuelve a arrancar los ventiladores que ha parado, así que en modo automático el portátil podía quedarse a **0 RPM con 95 °C** (issue [#175](https://github.com/yunusemreyl/omen-space/issues/175) del proyecto original).

Este fork corrige eso y ajusta el resto del programa para esta máquina concreta. Cada cambio se ha medido en el hardware real: volcados de registros del EC, la tabla de ventiladores de la BIOS, límites térmicos y lecturas de consumo y uso de CPU.

> [!IMPORTANT]
> Esta versión está **ajustada y probada solo en la placa `8C75`**. Para otros modelos OMEN, Victus o Transcend, usa el [proyecto original](https://github.com/yunusemreyl/omen-space).

### Equipo de pruebas

| | |
| :--- | :--- |
| **Portátil** | HP OMEN 17-db0xxx · placa `8C75` |
| **CPU / GPU** | AMD Ryzen 7 8845HS · NVIDIA GeForce RTX 4070 Laptop (híbrida) |
| **Pantalla** | Panel interno de 17,3" a 240 Hz (eDP) |
| **Sistema** | Nobara 44 · KDE Plasma 6 en Wayland · kernel 7.3 |

---

## Qué cambia

### 🌀 Refrigeración

- **Control de ventiladores que funciona en la 8C75.** El nivel de los ventiladores se escribe directamente en el EC (`XSS1`/`XSS2`). El modo Máx va por WMI, a unas 4500 RPM. Se ha recuperado la lectura de RPM en tiempo real.
- **Curva Auto estable.** Auto va por escalones, con 6 °C de margen antes de bajar de nivel, así que los ventiladores mantienen una velocidad constante en lugar de subir y bajar cada pocos segundos. Llegan a un 87 % antes de que la CPU alcance 90 °C. El 100 % llega con 90 °C en la CPU o 84 °C en la gráfica.
- **Modo Silencioso.** Curva silenciosa integrada, que además usa automáticamente el perfil Eco.
- **Ventiladores parados en reposo.** Los ventiladores se apagan del todo cuando el portátil está frío. Antes, un mínimo de 45 °C en la lectura de temperatura y los sensores de la RAM DDR5 los mantenían girando. Los sensores de la DDR5 ahora solo cuentan a partir de 75 °C.
- **Sigue al perfil de energía activo.** La curva de ventiladores sigue el perfil que tiene el kernel, también cuando KDE pasa a Eco al desconectar el cargador y vuelve al conectarlo.
- **Protección térmica** ante sobrecalentamiento sostenido, y el arreglo de temperatura de la gráfica, ambos traídos de la v2.2.1 original.

### ⚡ Energía

- **Límites reales de AMD.** Se muestran en modo solo lectura los límites STAPM y PPT reales de `amd-pmf`, en lugar de los valores de relleno de Intel (45/115 W). El firmware los vuelve a fijar en cada cambio de perfil, así que la página lo explica en lugar de ofrecer controles que no se mantendrían.
- **El escritorio manda en el perfil.** Conectar o desconectar el cargador ya no fuerza un perfil térmico ni muestra un aviso. Cuando KDE o power-profiles-daemon cambian el perfil, OMEN Space lo adopta: lo guarda y ajusta la potencia de la gráfica (TGP/PPAB).
- **Sin sondear la SMU.** `ryzenadj` nunca se consulta en bucle, porque sondear la SMU en este kernel podía congelar el sistema.
- **Sin controles que no hacen nada.** Curve Optimizer y el cuidado de la batería aparecen como no disponibles, porque el firmware los rechaza o Linux no tiene forma de usarlos. La página MUX solo ofrece los modos de gráfica que la BIOS acepta.

### 🖥️ Pantalla y teclado

- **60 Hz en Eco y 240 Hz en el resto.** El panel interno usa el modo de 60 Hz de `freesync_video` en Eco, que cambia sin pantallazo negro.
- **Brillo por debajo del mínimo del panel.** Por debajo del 8 % de la barra de KDE la luz de la pantalla ya casi no baja. En ese tramo se oscurece la imagen con un perfil de calibración (ICC/vcgt), del 100 % al 50 % en el 0 %. La luz nocturna sigue funcionando por encima.
- **Luz del teclado en KDE.** La luz del teclado se expone mediante `uleds`, así que la barra de brillo de Plasma y su indicador en pantalla funcionan con ella.

### 🪶 Eficiencia

- **Nada consulta cada X segundos.** El icono de la bandeja usa `ksni` 0.3 y reacciona a señales D-Bus del daemon, de power-profiles-daemon y de KDE. Ya no comprueba el estado cada 50 ms.
- **Un daemon más ligero.** Ya no revisa todos los procesos cada segundo. Solo calcula la telemetría en directo mientras una ventana o el overlay la muestran, y usa dos hilos. El uso de CPU en reposo bajó de un 2 % a menos de un 1 %.
- **Apagado limpio.** El daemon se cierra al instante con `SIGTERM`; antes tardaba 45 s y acababa en `SIGABRT`.

### 🧭 Comodidad

- Un clic en el icono de la bandeja abre la ventana, y otro clic la cierra.
- La gráfica apagada aparece como **Apagada** en lugar de `D3Cold`, y el estado del driver DKMS se muestra bien.
- Versión **2.2.1**, sin buscar actualizaciones en segundo plano. El actualizador del proyecto original está desactivado porque este es un fork personal: las versiones nuevas del original se revisan y se adaptan a mano.

---

## Notas de hardware (8C75)

Hallazgos recogidos al desarrollar este fork, útiles para cualquiera que trabaje con esta placa:

| Tema | Hallazgo |
| :--- | :--- |
| **Modo automático del EC** | `XSS1`/`XSS2` = `0xFF` deja el control al EC. El EC para los ventiladores en frío, pero nunca los vuelve a arrancar. |
| **Tabla de ventiladores de la BIOS** | 12 niveles fijos, de 1600 a 3900 RPM (de 22 a 46 dB), sin puntos de temperatura. El nivel más alto de HP es aproximadamente el 87 % del máximo por WMI. |
| **Disipador compartido** | Se probó a mover los dos ventiladores por separado y se descartó. Con 75 %/30 %, la CPU seguía subiendo por encima de 87 °C, frente a 79 °C con los dos al 75 %. |
| **Límites térmicos** | La CPU no se frena a 95 °C (61 W, ~4,2 GHz). La gráfica apunta a 87 °C, se frena a ~88 °C y se apaga a ~98 °C. |
| **Límites de potencia AMD** | Los fija la tabla estática de `amd-pmf` en cada cambio de perfil. Con cargador: Eco y Equilibrado 65/54/65 W; Rendimiento, PPT lento de 60 W. Con batería, mucho más bajos. |
| **MUX** | La BIOS solo acepta los modos 0 y 1. En HP, "híbrido" es Advanced Optimus en Windows. |
| **Retroiluminación** | Mínimo del firmware ≈ 2 % de la luz, alcanzado en el 6 % de la barra de KDE. |

---

## Instalación

> [!WARNING]
> Instala este fork solo en un HP OMEN 17-db0xxx (placa `8C75`). Puedes comprobar tu placa con `cat /sys/class/dmi/id/board_name`.

```bash
git clone https://github.com/pedrovb25/omen-space.git
cd omen-space
sudo ./setup.sh install
```

El instalador es el mismo que el del proyecto original. Instala las dependencias, compila todo y registra los drivers DKMS del kernel (`hp-wmi` y `hp-omen-extra`). Para quitarlo todo: `sudo ./setup.sh uninstall`.

<details>
<summary><b>Ramas</b></summary>
<br />

`main` es la línea estable de este fork. Cada cambio se desarrolló en su propia rama (`feat/8c75-*`, `fix/8c75-*`, `perf/*`…), cada una encima de la anterior, así que cada arreglo se puede revisar o aplicar por separado.
</details>

---

## Arquitectura

La arquitectura es la misma que la del proyecto original:

| Componente | Función |
| :--- | :--- |
| **`omen-space-daemon`** | Servicio `systemd` con permisos de root: WMI, ACPI, sysfs, EC, energía y HID mediante D-Bus (`org.hp.omen.*`). |
| **`omen-gui`** | Aplicación de escritorio en GTK4 + Libadwaita. |
| **`omen-tray`** | Icono de la bandeja: modos rápidos de ventilador y gráfica, frecuencia del panel según el perfil y oscurecimiento con brillo bajo. |
| **`omen-overlay`** | Panel de acceso rápido (`Shift+F2`). |
| **`omen-cli`** | Interfaz de línea de comandos para scripts. |
| **`hp-omen-extra`** | Paquete de drivers DKMS del kernel (`hp-wmi` + `hp-omen-extra`). |

---

## Créditos y licencia

Todo el mérito de OMEN Space es de sus autores; este fork solo lo adapta a una placa.

- **[yunusemreyl](https://github.com/yunusemreyl)**: desarrollador principal de OMEN Space
- **[tuxov](https://github.com/tuxov)**: responsable del módulo del kernel
- Los colaboradores de la comunidad de OMEN Space (ver el [proyecto original](https://github.com/yunusemreyl/omen-space#credits--license))
- **[pedrovb25](https://github.com/pedrovb25)**: fork para la 8C75, desarrollado con la ayuda de **Claude Opus 5.5** (Anthropic) para diagnosticar los problemas y revisar el código

Licencia **GPL-3.0**, igual que el proyecto original.

*Aviso: OMEN Space es un proyecto independiente de código abierto y no está afiliado ni respaldado por HP Inc.*
