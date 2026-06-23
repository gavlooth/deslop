(ns corpus.sloppy-clojure)

(defn bad-bools [x y z p]
  [(not (= x y))
   (not (nil? z))
   (if p true false)])

(defn redundant-wrapper [ready?]
  (when ready? (do (println "ready"))))

(defn collection-checks [xs]
  [(= (count xs) 0)
   (> (count xs) 0)
   (reduce conj [] xs)])

(defn single-use [x]
  (let [answer (+ x 1)] answer))
