/*
 * QuailSync ESP32-C3 Super Mini — DHT22 Sensor Node
 *
 * Reads temperature/humidity from a DHT22 sensor and sends readings
 * to the QuailSync server via WebSocket.
 *
 * Required Arduino libraries (install via Library Manager):
 *   - "DHT sensor library" by Adafruit
 *   - "Adafruit Unified Sensor" by Adafruit (dependency of DHT library)
 *   - "ArduinoWebsockets" by Gil Maimon
 *   - "ArduinoJson" by Benoit Blanchon
 *
 * Board: ESP32C3 Dev Module (Tools > Board > esp32 > ESP32C3 Dev Module)
 * USB CDC On Boot: Enabled (for Serial Monitor output)
 *
 * Wiring:
 *   ESP32-C3       DHT22
 *   3.3V --------  + (VCC)
 *   GND  --------  - (GND)
 *   GPIO4 -------  out (DATA)
 *   10kΩ resistor between DATA and VCC (pull-up)
 */

#include <WiFi.h>
#include <ArduinoWebsockets.h>
#include <ArduinoJson.h>
#include <DHT.h>

using namespace websockets;

// ======================== CONFIGURATION ========================
// Change these to match your setup

const char* WIFI_SSID     = "";       // <-- Change to your WiFi network name
const char* WIFI_PASSWORD = "";    // <-- Change to your WiFi password
const char* SERVER_HOST   = "";       // <-- QuailSync server IP
const int   SERVER_PORT   = 3000;                  // <-- QuailSync server port
const int   BROODER_ID    = 4;                     // <-- Unique per sensor node (1, 2, 3...)

// ======================== PIN CONFIG ===========================

#define DHTPIN    4
#define DHTTYPE   DHT22
#define LED_PIN   8    // Onboard LED on ESP32-C3 Super Mini

// ======================== TIMING ===============================

const unsigned long READ_INTERVAL_MS    = 5000;   // Read sensor every 5 seconds
const unsigned long WS_RETRY_DELAY_MS   = 5000;   // Retry WebSocket connection every 5 seconds
const unsigned long WIFI_CHECK_INTERVAL = 10000;  // Check WiFi every 10 seconds

// ======================== GLOBALS ==============================

DHT dht(DHTPIN, DHTTYPE);
WebsocketsClient wsClient;

bool wsConnected = false;
unsigned long lastReadTime = 0;
unsigned long lastWifiCheck = 0;
unsigned long lastWsAttempt = 0;
int successCount = 0;
int errorCount = 0;

// Build the WebSocket URL from host and port
String wsUrl;

// ======================== LED HELPERS ==========================

void ledOn()  { digitalWrite(LED_PIN, LOW); }   // Active low on ESP32-C3 Super Mini
void ledOff() { digitalWrite(LED_PIN, HIGH); }

void blinkSuccess() {
  ledOn();
  delay(50);
  ledOff();
}

void blinkError() {
  for (int i = 0; i < 3; i++) {
    ledOn();
    delay(80);
    ledOff();
    delay(80);
  }
}

// ======================== WIFI =================================

void connectWiFi() {
  if (WiFi.status() == WL_CONNECTED) return;

  Serial.printf("[wifi] Connecting to %s", WIFI_SSID);
  WiFi.mode(WIFI_STA);
  WiFi.begin(WIFI_SSID, WIFI_PASSWORD);

  int attempts = 0;
  while (WiFi.status() != WL_CONNECTED && attempts < 40) {
    delay(500);
    Serial.print(".");
    attempts++;
  }

  if (WiFi.status() == WL_CONNECTED) {
    Serial.printf("\n[wifi] Connected! IP: %s\n", WiFi.localIP().toString().c_str());
  } else {
    Serial.println("\n[wifi] Connection failed — will retry");
    blinkError();
  }
}

void checkWiFi() {
  unsigned long now = millis();
  if (now - lastWifiCheck < WIFI_CHECK_INTERVAL) return;
  lastWifiCheck = now;

  if (WiFi.status() != WL_CONNECTED) {
    Serial.println("[wifi] Disconnected — reconnecting...");
    WiFi.disconnect();
    connectWiFi();
  }
}

// ======================== WEBSOCKET ============================

void onWsMessage(WebsocketsMessage message) {
  Serial.printf("[ws] Received: %s\n", message.data().c_str());
}

void onWsEvent(WebsocketsEvent event, String data) {
  switch (event) {
    case WebsocketsEvent::ConnectionOpened:
      Serial.println("[ws] Connected to server");
      wsConnected = true;
      break;
    case WebsocketsEvent::ConnectionClosed:
      Serial.println("[ws] Disconnected from server");
      wsConnected = false;
      break;
    case WebsocketsEvent::GotPing:
      break;
    case WebsocketsEvent::GotPong:
      break;
  }
}

void connectWebSocket() {
  if (wsConnected) return;
  if (WiFi.status() != WL_CONNECTED) return;

  unsigned long now = millis();
  if (now - lastWsAttempt < WS_RETRY_DELAY_MS) return;
  lastWsAttempt = now;

  Serial.printf("[ws] Connecting to %s ...\n", wsUrl.c_str());

  wsClient.onMessage(onWsMessage);
  wsClient.onEvent(onWsEvent);

  bool connected = wsClient.connect(wsUrl);
  if (!connected) {
    Serial.println("[ws] Connection failed — retrying in 5s");
    blinkError();
  }
}

// ======================== TIMESTAMP ============================

String getTimestamp() {
  // ISO 8601 UTC timestamp from NTP-synced system clock
  struct tm timeinfo;
  if (getLocalTime(&timeinfo, 0)) {
    char buf[25];
    strftime(buf, sizeof(buf), "%Y-%m-%dT%H:%M:%SZ", &timeinfo);
    return String(buf);
  }
  // Fallback if NTP hasn't synced yet: use millis-based uptime
  // The server records its own received_at, so this is acceptable
  unsigned long s = millis() / 1000;
  char buf[25];
  snprintf(buf, sizeof(buf), "2026-01-01T%02lu:%02lu:%02luZ",
           (s / 3600) % 24, (s / 60) % 60, s % 60);
  return String(buf);
}

// ======================== SENSOR & SEND ========================

void readAndSend() {
  unsigned long now = millis();
  if (now - lastReadTime < READ_INTERVAL_MS) return;
  lastReadTime = now;

  float humidity = dht.readHumidity();
  float tempC = dht.readTemperature();  // Celsius

  if (isnan(humidity) || isnan(tempC)) {
    Serial.println("[dht] Read failed — check wiring");
    errorCount++;
    blinkError();
    return;
  }

  // Convert to Fahrenheit (the server field is named temperature_celsius
  // but QuailSync stores and displays Fahrenheit — matches Pi agent behavior)
  float tempF = tempC * 9.0 / 5.0 + 32.0;

  Serial.printf("[dht] Temp: %.1fF (%.1fC)  Humidity: %.1f%%\n", tempF, tempC, humidity);

  if (!wsConnected) {
    Serial.println("[ws] Not connected — reading discarded");
    return;
  }

  // Build JSON payload matching TelemetryPayload::Brooder
  JsonDocument doc;
  JsonObject brooder = doc["Brooder"].to<JsonObject>();
  brooder["temperature_celsius"] = round(tempF * 10.0) / 10.0;
  brooder["humidity_percent"]    = round(humidity * 10.0) / 10.0;
  brooder["timestamp"]           = getTimestamp();
  brooder["brooder_id"]          = BROODER_ID;

  String json;
  serializeJson(doc, json);

  bool sent = wsClient.send(json);
  if (sent) {
    successCount++;
    Serial.printf("[ws] Sent: %s (ok:%d err:%d)\n", json.c_str(), successCount, errorCount);
    blinkSuccess();
  } else {
    errorCount++;
    Serial.println("[ws] Send failed");
    blinkError();
    wsConnected = false;  // Force reconnect on next loop
  }
}

// ======================== SETUP & LOOP =========================

void setup() {
  Serial.begin(115200);
  delay(1000);

  pinMode(LED_PIN, OUTPUT);
  ledOff();

  // Build WebSocket URL from config
  wsUrl = "ws://" + String(SERVER_HOST) + ":" + String(SERVER_PORT) + "/ws";

  Serial.println();
  Serial.println("================================");
  Serial.println("  QuailSync ESP32-C3 Sensor");
  Serial.printf("  Brooder ID: %d\n", BROODER_ID);
  Serial.printf("  Server:     %s:%d\n", SERVER_HOST, SERVER_PORT);
  Serial.printf("  DHT22 pin:  GPIO%d\n", DHTPIN);
  Serial.printf("  Interval:   %lums\n", READ_INTERVAL_MS);
  Serial.println("================================");

  dht.begin();
  connectWiFi();

  // Sync time via NTP (UTC, no offset)
  configTime(0, 0, "pool.ntp.org", "time.nist.gov");
  Serial.println("[ntp] Time sync requested (UTC)");
}

void loop() {
  checkWiFi();
  connectWebSocket();

  if (wsConnected) {
    wsClient.poll();
  }

  readAndSend();
}
