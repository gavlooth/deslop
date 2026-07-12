(ns control-edges)

(defn classify [value]
  (if (positive? value)
    (when (> value 10)
      :large)
    :non-positive))

(defn ordinary [value]
  (println (inc value))
  (+ value 1))

(defmacro guarded [test & body]
  `(when ~test
     ~@body))

(defn quoted-and-discarded [value]
  '(if hidden
     (when secret :hidden)
     :quoted)
  #_(when discarded :ignored)
  (guarded value
    (if value :live :empty)))

(defn consume [values]
  (loop [remaining values]
    (if (seq remaining)
      (recur (rest remaining))
      :done)))
