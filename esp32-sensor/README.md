# QuailSync ESP32-C3 Super Mini — DHT22 Sensor Node

Wireless temperature/humidity sensor that sends readings to QuailSync via WebSocket.

## Wiring

```
ESP32-C3 Super Mini          DHT22
┌─────────────────┐     ┌───────────┐
│            3.3V ├─────┤ + (VCC)   │
│                 │  ┌──┤ out (DATA)│
│           GPIO4 ├──┤  │ - (GND)  │
│             GND ├──┼──┤           │
└─────────────────┘  │  └───────────┘
                     │
                 [10kΩ] ← pull-up resistor
                     │
                   3.3V
```

- **3.3V** → DHT22 **+** (VCC)
- **GPIO4** → DHT22 **out** (DATA)
- **GND** → DHT22 **-** (GND)
- **10kΩ pull-up** resistor between DATA and 3.3V

## Arduino IDE Setup

### 1. Install ESP32 Board Support

1. Open **File > Preferences**
2. Add to "Additional Board Manager URLs":
   ```
   https://raw.githubusercontent.com/espressif/arduino-esp32/gh-pages/package_esp32_index.json
   ```
3. Open **Tools > Board > Boards Manager**
4. Search "esp32" and install **esp32 by Espressif Systems**

### 2. Board Settings

- **Tools > Board**: ESP32C3 Dev Module
- **Tools > USB CDC On Boot**: Enabled (required for Serial Monitor output)
- **Tools > Port**: Select the COM port for your ESP32-C3

### 3. Install Libraries

Open **Tools > Manage Libraries** and install these four libraries:

1. **DHT sensor library** by Adafruit
2. **Adafruit Unified Sensor** by Adafruit (dependency — install this too)
3. **ArduinoWebsockets** by Gil Maimon
4. **ArduinoJson** by Benoit Blanchon

### 4. Configure

Open `esp32-sensor.ino` and edit the constants at the top:

```cpp
const char* WIFI_SSID     = "YourWiFiSSID";
const char* WIFI_PASSWORD = "YourWiFiPassword";
const char* SERVER_HOST   = "192.168.0.114";
const int   SERVER_PORT   = 3000;
const int   BROODER_ID    = 1;  // unique per sensor node
```

### 5. Flash

1. Connect the ESP32-C3 via USB-C
2. Click **Upload** (→ button)
3. Open **Serial Monitor** at **115200** baud to verify output

## Multiple Sensor Nodes

Each ESP32-C3 needs a unique `BROODER_ID`. Flash each board with a different value:

- Brooder 1 sensor: `BROODER_ID = 1`
- Brooder 2 sensor: `BROODER_ID = 2`
- Brooder 3 sensor: `BROODER_ID = 3`

## LED Status

| Pattern | Meaning |
|---------|---------|
| Single short blink | Reading sent successfully |
| Rapid 3x blink | Error (sensor read fail, WiFi issue, or WebSocket disconnect) |

## Troubleshooting

- **No serial output**: Make sure "USB CDC On Boot" is set to "Enabled" in board settings
- **DHT read failed**: Check wiring, ensure 10kΩ pull-up resistor is between DATA and 3.3V
- **WiFi won't connect**: Verify SSID/password, must be a 2.4GHz network (ESP32-C3 doesn't support 5GHz)
- **WebSocket won't connect**: Verify SERVER_HOST IP and that QuailSync is running on the specified port
