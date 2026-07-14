;; @generated
(ns adapter-matrix)

^:generated
(defn compute [π values]
  ; line comment
  (let [total (* π 2)]
    (if (> total 10)
      (map inc values)
      (do
        '(if quoted (when hidden :bad) :data)
        #_(when discarded :bad)
        #?(:clj (println total) :cljs (js/console.log total))
        `(when ~π ~@values)
        #=(+ 1 2)
        total))))
