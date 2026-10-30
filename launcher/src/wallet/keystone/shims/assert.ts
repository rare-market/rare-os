/**
 * Browser stand-in for Node's `assert`, which monero-ts requires (assert, .equal, .deepEqual).
 * Aliased in vite.config.ts; only the offscreen Monero chunk pulls it in.
 */
class AssertionError extends Error {
  name = 'AssertionError'
}

function assert(value: unknown, message?: string | Error): asserts value {
  if (!value) throw message instanceof Error ? message : new AssertionError(message ?? 'Assertion failed')
}

const fail = (message: string | Error | undefined, fallback: string): never => {
  throw message instanceof Error ? message : new AssertionError(message ?? fallback)
}

assert.ok = assert
assert.equal = (a: unknown, b: unknown, message?: string | Error) => {
  // eslint-disable-next-line eqeqeq
  if (!(a == b)) fail(message, `${String(a)} == ${String(b)}`)
}
assert.notEqual = (a: unknown, b: unknown, message?: string | Error) => {
  // eslint-disable-next-line eqeqeq
  if (a == b) fail(message, `${String(a)} != ${String(b)}`)
}
assert.strictEqual = (a: unknown, b: unknown, message?: string | Error) => {
  if (a !== b) fail(message, `${String(a)} === ${String(b)}`)
}
const json = (v: unknown) => JSON.stringify(v, (_k, x) => (typeof x === 'bigint' ? x.toString() + 'n' : x))
assert.deepEqual = (a: unknown, b: unknown, message?: string | Error) => {
  if (json(a) !== json(b)) fail(message, 'Values are not deeply equal')
}
assert.deepStrictEqual = assert.deepEqual
assert.fail = (message?: string | Error) => fail(message, 'Failed')
assert.AssertionError = AssertionError

export default assert
export { AssertionError }
