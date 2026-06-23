(ns fixture.clean
  (:require [clojure.string :as str]
            [clojure.set :as set]))

(defrecord CreateUser [id name email])
(defrecord UpdateUser [id name email])

(def schema-a
  {:id string?
   :name string?
   :email string?})

(def schema-b
  {:id string?
   :name string?
   :email string?})

(defn normalize-user [user]
  (update user :name str/trim))
