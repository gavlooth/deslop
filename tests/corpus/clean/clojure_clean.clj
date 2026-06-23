(ns corpus.clean-clojure
  (:require [clojure.string :as str]
            [clojure.set :as set]))

(def default-options
  {:retry 2
   :timeout-ms 100})

(def fallback-options
  {:retry 2
   :timeout-ms 100})

(defn clean-checks [xs]
  [(empty? xs)
   (seq xs)
   (vec xs)])

(defn reused-binding [x]
  (let [answer (+ x 1)]
    (* answer answer)))

