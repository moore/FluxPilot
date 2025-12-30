class AsyncQueue {
  constructor() {
    console.log("constructing AsyncQueue");
    this.values = [];
    this.resolvers = [];
  }

  enqueue(value) {
    console.log("enqueue", value);
    console.log("this.resolvers.length", this.resolvers.length);
    if (this.resolvers.length > 0) {
      const resolve = this.resolvers.shift();
      console.log("resolving!!!");
      resolve(value);
    } else {
        console.log("pushing");
      this.values.push(value);
    }
  }

  dequeue() {
    if (this.values.length > 0) {
        const value = this.values.shift();
        console.log("got value", value);
        return Promise.resolve(value);
    } else {
        console.log("enquing resolver");
        return new Promise((resolve) => {
            console.log("promise actavated");
            this.resolvers.push(resolve);
            console.log("resolvers len()", this.resolvers.length);
        });
    }
  }
  // Can add a close() method and [Symbol.asyncIterator] for 'for await' loops
}

export default AsyncQueue;
