type Path = string[]
type ChangeCallback = (newValue: unknown, path: Path) => void

// Read fields from the current snapshot and send assignments through the write callback.
export const makeObservable = <T extends object>(rootObject: T, readObject: () => object | null, callback: ChangeCallback, readParent: (id: number) => object | null): T => {
    // Resolve the current object and reject deleted objects or missing intermediate fields in one place.
    const getCurrentObject = (path: Path): object => {
        let val: unknown = readObject()
        let index = 0
        while (true) {
            if (val === null || typeof val !== 'object') throw new Error('Object or field no longer exists')
            if (index === path.length) return val
            const nextValueAtPath = Reflect.get(val, path[index])
            val = nextValueAtPath
            index += 1
        }
    }

    // Proxies belong to field paths, so replacing a nested object does not leave a cached reference behind.
    const createProxy = <U extends object>(object: U, path: Path): U => {
        const proxyOfKey: { [key: string]: object } = Object.create(null)
        // Preserve the target's required keys: ID, array length, and the object's helper methods.
        const requiredKeys = Reflect.ownKeys(object).filter(key => Reflect.getOwnPropertyDescriptor(object, key)?.configurable === false)

        const proxy = new Proxy(object, {
            get: (obj, prop, receiver) => {
                const localValue = Reflect.get(obj, prop, receiver)
                const descriptor = Reflect.getOwnPropertyDescriptor(obj, prop)
                if (typeof prop !== 'string' || typeof localValue === 'function' || descriptor?.writable === false) return localValue
                const subpath = [...path, prop]
                const subobj = Reflect.get(getCurrentObject(path), prop)
                // Parent is stored as an ID in Rust and exposed as a live object in scripts.
                if (path.length === 0 && prop === 'parent') return subobj === null ? null : readParent(subobj)
                if (subobj !== null && typeof subobj === 'object') {
                    if (!Object.hasOwn(proxyOfKey, prop) || Array.isArray(proxyOfKey[prop]) !== Array.isArray(subobj)) proxyOfKey[prop] = createProxy(subobj, subpath)
                    return proxyOfKey[prop]
                }
                return subobj
            },
            set: (_obj, prop, value) => {
                if (typeof prop !== 'string') return false
                callback(path.length === 0 && prop === 'parent' ? value.id : value, [...path, prop])
                return true
            },
            deleteProperty: (_obj, prop) => {
                if (typeof prop !== 'string') return false
                callback(undefined, [...path, prop])
                return true
            },
            // Enumeration must see changed optional fields and arrays, including during JSON serialization.
            ownKeys: () => [...new Set([...Reflect.ownKeys(getCurrentObject(path)), ...requiredKeys])],
            has: (_obj, prop) => Reflect.has(getCurrentObject(path), prop) || requiredKeys.includes(prop),
            getOwnPropertyDescriptor: (obj, prop) => {
                const descriptor = Reflect.getOwnPropertyDescriptor(obj, prop)
                if (descriptor?.configurable === false && descriptor.writable === false) return descriptor
                return Reflect.getOwnPropertyDescriptor(getCurrentObject(path), prop)
            },
            // Engine fields are changed through assignment, not by redefining the JS view.
            defineProperty: () => false,
            preventExtensions: () => false,
            setPrototypeOf: () => false,
        })

        return proxy
    }
    return createProxy(rootObject, [])
}
