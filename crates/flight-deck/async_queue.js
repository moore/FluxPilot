class AsyncQueue {
  constructor() {
    this.values = [];
    this.resolvers = [];
  }

  enqueue(value) {
   if (this.resolvers.length > 0) {
      const resolve = this.resolvers.shift();
      resolve(value);
    } else {
      this.values.push(value);
    }
  }

  dequeue() {
    if (this.values.length > 0) {
        const value = this.values.shift();
        return Promise.resolve(value);
    } else {
        return new Promise((resolve) => {
            this.resolvers.push(resolve);
        });
    }
  }
  // Can add a close() method and [Symbol.asyncIterator] for 'for await' loops
}

export default AsyncQueue;
