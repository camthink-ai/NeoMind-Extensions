// NeoMind AssemblyScript Extension
// A simple WASM extension demonstrating cross-platform extension development

// ============================================================================
// Constants
// ============================================================================

const ABI_VERSION: u32 = 2;
const SUCCESS_TRUE: string = "true";
const SUCCESS_FALSE: string = "false";

// Extension state
let counter: i32 = 42;
let temperature: f64 = 23.5;
let humidity: f64 = 65.0;

// ============================================================================
// FFI Exports - ABI Version
// ============================================================================

export function neomind_extension_abi_version(): u32 {
  return ABI_VERSION;
}

export function getAbiVersion(): u32 {
  return ABI_VERSION;
}

// ============================================================================
// Metrics - Getter Functions
// ============================================================================

export function get_counter(): i32 {
  return counter;
}

export function increment_counter(): i32 {
  counter = counter + 1;
  return counter;
}

export function reset_counter(): void {
  counter = 42;
}

export function get_temperature(): f64 {
  return temperature;
}

export function set_temperature(temp: f64): void {
  temperature = temp;
}

export function get_humidity(): f64 {
  return humidity;
}

export function set_humidity(hum: f64): void {
  humidity = hum;
}

// ============================================================================
// Commands
// ============================================================================

export function neomind_execute(
  command_ptr: usize,
  args_ptr: usize,
  result_buf_ptr: usize,
  result_buf_len: usize
): usize {
  const command = readString(command_ptr);
  let response: string;

  // Simple command matching using if-else
  if (command == "get_counter") {
    response = buildJsonResponse(
      "Counter retrieved",
      "counter",
      counter.toString(),
      "count"
    );
  } else if (command == "increment_counter") {
    counter = counter + 1;
    response = buildJsonResponse(
      "Counter incremented",
      "counter",
      counter.toString(),
      "count"
    );
  } else if (command == "reset_counter") {
    counter = 42;
    response = buildJsonResponse(
      "Counter reset to default",
      "counter",
      "42",
      "count"
    );
  } else if (command == "get_temperature") {
    response = buildJsonResponse(
      "Temperature reading",
      "temperature",
      temperature.toString(),
      "°C"
    );
  } else if (command == "get_humidity") {
    response = buildJsonResponse(
      "Humidity reading",
      "humidity",
      humidity.toString(),
      "%"
    );
  } else if (command == "set_temperature") {
    const argsStr = readString(args_ptr);
    const temp = parseFloat(argsStr);
    if (!isNaN(temp)) {
      temperature = temp;
      response = buildJsonResponse(
        "Temperature updated",
        "temperature",
        temperature.toString(),
        "°C"
      );
    } else {
      response = buildJsonError("Invalid temperature value", "Parsing error");
    }
  } else if (command == "hello") {
    response = '{"success":true,"message":"Hello from AssemblyScript WASM Extension!","extension":"as-hello","version":"0.3.0","language":"AssemblyScript","wasm":true}';
  } else if (command == "get_all_metrics") {
    response = buildAllMetricsResponse();
  } else {
    response = buildUnknownCommandResponse(command);
  }

  return writeString(result_buf_ptr, result_buf_len, response);
}

export function hello(): usize {
  const response = '{"success":true,"message":"Hello from AssemblyScript!","version":"0.3.0"}';
  return response.length;
}

export function health(): i32 {
  return 1;
}

// ============================================================================
// Helper Functions
// ============================================================================

function readString(ptr: usize): string {
  if (ptr === 0) return "";

  let len: i32 = 0;
  const maxLen: i32 = 1000;
  while (len < maxLen && load<u8>(ptr + usize(len)) !== 0) {
    len = len + 1;
  }

  // Manual UTF-8 decoding (simplified for ASCII)
  let result = "";
  for (let i: i32 = 0; i < len; i = i + 1) {
    const c = load<u8>(ptr + usize(i));
    if (c !== 0) {
      result = result + String.fromCharCode(c);
    }
  }
  return result;
}

function writeString(ptr: usize, maxLen: usize, str: string): usize {
  const len: usize = str.length;
  const writeLen: usize = select<usize>(len < maxLen, len, maxLen - 1);

  // Write bytes
  for (let i: usize = 0; i < writeLen; i = i + 1) {
    const code: i32 = str.charCodeAt(i);
    const c: u8 = <u8>code;
    store<u8>(ptr + i, c);
  }

  // Null terminate
  store<u8>(ptr + writeLen, 0);

  return writeLen;
}

// Select function for conditional (AssemblyScript doesn't support ternary with different types)
@inline
function select<T>(cond: bool, ifTrue: T, ifFalse: T): T {
  return cond ? ifTrue : ifFalse;
}

function buildJsonResponse(message: string, name: string, value: string, unit: string): string {
  return '{"success":true,"message":"' + message + '","data":{"name":"' + name + '","value":' + value + ',"unit":"' + unit + '"}}';
}

function buildJsonError(message: string, error: string): string {
  return '{"success":false,"message":"' + message + '","error":"' + error + '"}';
}

function buildUnknownCommandResponse(command: string): string {
  return '{"success":false,"message":"Unknown command","error":"Command not found","available_commands":["get_counter","increment_counter","reset_counter","get_temperature","set_temperature","get_humidity","hello","get_all_metrics"]}';
}

function buildAllMetricsResponse(): string {
  return '{"success":true,"message":"All metrics retrieved","data":{"counter":' + counter.toString() + ',"temperature":' + temperature.toString() + ',"humidity":' + humidity.toString() + '}}';
}
