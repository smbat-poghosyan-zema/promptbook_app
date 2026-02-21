type Parser<T> = (input: unknown, path?: string) => T;

class Schema<T> {
  protected parser: Parser<T>;

  constructor(parser: Parser<T>) {
    this.parser = parser;
  }

  parse(input: unknown): T {
    return this.parser(input, "$input");
  }

  optional(): Schema<T | undefined> {
    return new Schema<T | undefined>((input, path) => {
      if (input === undefined) {
        return undefined;
      }

      return this.parser(input, path);
    });
  }
}

class StringSchema extends Schema<string> {
  min(minLength: number): StringSchema {
    return new StringSchema((input, path) => {
      const value = this.parser(input, path);
      if (value.length < minLength) {
        throw new Error(`${path ?? "$input"} must have at least ${minLength} characters`);
      }

      return value;
    });
  }

  uuid(): StringSchema {
    return new StringSchema((input, path) => {
      const value = this.parser(input, path);
      const uuidPattern =
        /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

      if (!uuidPattern.test(value)) {
        throw new Error(`${path ?? "$input"} must be a valid UUID`);
      }

      return value;
    });
  }
}

type ObjectShape = Record<string, Schema<unknown>>;

type InferObject<TShape extends ObjectShape> = {
  [K in keyof TShape]: TShape[K] extends Schema<infer TValue> ? TValue : never;
};

function string(): StringSchema {
  return new StringSchema((input, path) => {
    if (typeof input !== "string") {
      throw new Error(`${path ?? "$input"} must be a string`);
    }

    return input;
  });
}

function literal<TLiteral extends string | number | boolean>(expected: TLiteral): Schema<TLiteral> {
  return new Schema<TLiteral>((input, path) => {
    if (input !== expected) {
      throw new Error(`${path ?? "$input"} must equal ${String(expected)}`);
    }

    return expected;
  });
}

function enumValue<TValues extends readonly string[]>(values: TValues): Schema<TValues[number]> {
  return new Schema<TValues[number]>((input, path) => {
    if (typeof input !== "string" || !values.includes(input)) {
      throw new Error(`${path ?? "$input"} must be one of: ${values.join(", ")}`);
    }

    return input as TValues[number];
  });
}

function array<TItem>(itemSchema: Schema<TItem>): Schema<TItem[]> {
  return new Schema<TItem[]>((input, path) => {
    if (!Array.isArray(input)) {
      throw new Error(`${path ?? "$input"} must be an array`);
    }

    return input.map((item, index) => itemSchema.parseWithPath(item, `${path ?? "$input"}[${index}]`));
  });
}

function object<TShape extends ObjectShape>(shape: TShape): Schema<InferObject<TShape>> {
  return new Schema<InferObject<TShape>>((input, path) => {
    if (input === null || typeof input !== "object" || Array.isArray(input)) {
      throw new Error(`${path ?? "$input"} must be an object`);
    }

    const record = input as Record<string, unknown>;
    const out: Record<string, unknown> = {};

    for (const [key, schema] of Object.entries(shape)) {
      out[key] = schema.parseWithPath(record[key], `${path ?? "$input"}.${key}`);
    }

    return out as InferObject<TShape>;
  });
}

Schema.prototype.parseWithPath = function parseWithPath<TValue>(
  this: Schema<TValue>,
  input: unknown,
  path: string
): TValue {
  return this.parser(input, path);
};

declare module "./index" {
  interface Schema<T> {
    parseWithPath(input: unknown, path: string): T;
  }
}

export const z = {
  string,
  literal,
  enum: enumValue,
  array,
  object
};

export namespace z {
  export type infer<TSchema extends Schema<unknown>> = TSchema extends Schema<infer TValue>
    ? TValue
    : never;
}
